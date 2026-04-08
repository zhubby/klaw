use crate::{
    event::{AppEvent, map_key_event},
    state::{AppState, TuiMeta},
    view,
};
use crossterm::{
    event::{Event as CrosstermEvent, EventStream},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use klaw_channel::{
    ChannelRequest, ChannelResult, ChannelRuntime, ChannelStreamEvent, ChannelStreamWriter,
    render::{OutputRenderStyle, render_agent_output},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{collections::BTreeMap, io};

pub async fn run_tui(meta: TuiMeta, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
    let mut terminal = setup_terminal()?;
    let result = run_loop(&mut terminal, AppState::new(meta), runtime).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut state: AppState,
    runtime: &dyn ChannelRuntime,
) -> ChannelResult<()> {
    let mut events = EventStream::new();
    let mut redraw_tick = tokio::time::interval(std::time::Duration::from_millis(200));
    let mut cron_tick = tokio::time::interval(runtime.cron_tick_interval());
    let mut runtime_tick = tokio::time::interval(runtime.runtime_tick_interval());

    loop {
        terminal.draw(|frame| view::render(frame, &state))?;

        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(CrosstermEvent::Key(key))) => {
                        let Some(event) = map_key_event(key) else {
                            continue;
                        };
                        if handle_app_event(event, &mut state, runtime).await? {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        state.push_error(format!("event error: {err}"));
                    }
                    None => break,
                }
            }
            _ = redraw_tick.tick() => {}
            _ = cron_tick.tick() => {
                runtime.on_cron_tick().await;
            }
            _ = runtime_tick.tick() => {
                runtime.on_runtime_tick().await;
            }
            signal = shutdown_signal() => {
                signal?;
                break;
            }
        }
    }

    Ok(())
}

async fn handle_app_event(
    event: AppEvent,
    state: &mut AppState,
    runtime: &dyn ChannelRuntime,
) -> ChannelResult<bool> {
    match event {
        AppEvent::Quit => Ok(true),
        AppEvent::ClearMessages => {
            state.clear_messages();
            Ok(false)
        }
        AppEvent::NewSession => {
            state.reset_session();
            Ok(false)
        }
        AppEvent::ToggleReasoning => {
            state.toggle_reasoning();
            Ok(false)
        }
        AppEvent::Backspace => {
            state.backspace();
            Ok(false)
        }
        AppEvent::Newline => {
            state.insert_newline();
            Ok(false)
        }
        AppEvent::Input(ch) => {
            state.insert_char(ch);
            Ok(false)
        }
        AppEvent::Submit => {
            let Some(input) = state.take_submit_input() else {
                return Ok(false);
            };
            state.begin_agent_response();
            let request = ChannelRequest {
                channel: state.meta().channel.clone(),
                input,
                session_key: state.meta().session_key.clone(),
                chat_id: state.chat_id(),
                media_references: Vec::new(),
                metadata: BTreeMap::new(),
            };
            let mut writer = InlineStreamWriter { state };
            let maybe_output = runtime.submit_streaming(request, &mut writer).await?;
            match maybe_output {
                Some(output) => {
                    let show_reasoning = writer.state.meta().show_reasoning;
                    let rendered =
                        render_agent_output(&output, show_reasoning, OutputRenderStyle::Terminal);
                    writer.state.apply_agent_snapshot(rendered);
                    writer.state.complete_agent_response();
                }
                None => {
                    writer.state.clear_pending_agent_response();
                    writer.state.push_error("[no response]");
                }
            }
            Ok(false)
        }
    }
}

struct InlineStreamWriter<'a> {
    state: &'a mut AppState,
}

#[async_trait::async_trait(?Send)]
impl ChannelStreamWriter for InlineStreamWriter<'_> {
    async fn write(&mut self, event: ChannelStreamEvent) -> ChannelResult<()> {
        match event {
            ChannelStreamEvent::Snapshot(output) => {
                let rendered = render_agent_output(
                    &output,
                    self.state.meta().show_reasoning,
                    OutputRenderStyle::Terminal,
                );
                self.state.apply_agent_snapshot(rendered);
            }
            ChannelStreamEvent::Clear => {
                self.state.apply_agent_snapshot(String::new());
            }
        }
        Ok(())
    }
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

async fn shutdown_signal() -> io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => signal,
                _ = terminate.recv() => Ok(()),
            }
        } else {
            tokio::signal::ctrl_c().await
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

#[cfg(test)]
mod tests {
    use super::handle_app_event;
    use crate::{
        event::AppEvent,
        state::{AppState, TuiMeta},
    };
    use klaw_channel::{
        ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime, ChannelStreamEvent,
        ChannelStreamWriter,
    };
    use std::{cell::Cell, collections::BTreeMap, time::Duration};

    #[derive(Default)]
    struct FakeRuntime {
        streaming_called: Cell<bool>,
    }

    #[async_trait::async_trait(?Send)]
    impl ChannelRuntime for FakeRuntime {
        async fn submit(&self, _request: ChannelRequest) -> ChannelResult<Option<ChannelResponse>> {
            panic!("submit should not be called when streaming path is used")
        }

        async fn submit_streaming(
            &self,
            _request: ChannelRequest,
            writer: &mut dyn ChannelStreamWriter,
        ) -> ChannelResult<Option<ChannelResponse>> {
            self.streaming_called.set(true);
            let response = ChannelResponse {
                content: "done".to_string(),
                reasoning: Some("step1".to_string()),
                metadata: BTreeMap::new(),
                attachments: Vec::new(),
            };
            writer
                .write(ChannelStreamEvent::Snapshot(response.clone()))
                .await?;
            Ok(Some(response))
        }

        fn cron_tick_interval(&self) -> Duration {
            Duration::from_secs(60)
        }

        fn runtime_tick_interval(&self) -> Duration {
            Duration::from_secs(60)
        }

        async fn on_cron_tick(&self) {}

        async fn on_runtime_tick(&self) {}
    }

    fn sample_meta() -> TuiMeta {
        TuiMeta {
            version: "0.10.3".to_string(),
            session_key: "terminal:test".to_string(),
            channel: "terminal".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            skills: "-".to_string(),
            tools: "-".to_string(),
            mcp: "-".to_string(),
            show_reasoning: false,
        }
    }

    #[tokio::test]
    async fn submit_event_uses_streaming_runtime_path() {
        let runtime = FakeRuntime::default();
        let mut state = AppState::new(sample_meta());
        state.set_input("hello".to_string());

        let should_quit = handle_app_event(AppEvent::Submit, &mut state, &runtime)
            .await
            .expect("submit should succeed");

        assert!(!should_quit);
        assert!(runtime.streaming_called.get());
        assert_eq!(state.messages().len(), 2);
        assert!(state.messages()[1].content.contains("[answer]"));
        assert!(state.messages()[1].content.contains("done"));
    }
}
