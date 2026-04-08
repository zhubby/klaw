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
use ratatui::{Terminal, backend::Backend};
use std::{cell::RefCell, collections::BTreeMap, io, rc::Rc};

pub async fn run_tui(meta: TuiMeta, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
    let state = Rc::new(RefCell::new(AppState::new(meta)));
    let term = Rc::new(RefCell::new(setup_terminal()?));
    let result = run_loop(Rc::clone(&term), Rc::clone(&state), runtime).await;
    {
        let mut t = term.borrow_mut();
        restore_terminal(&mut *t)?;
    }
    result
}

async fn run_loop(
    term: Rc<RefCell<Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>>>,
    state: Rc<RefCell<AppState>>,
    runtime: &dyn ChannelRuntime,
) -> ChannelResult<()> {
    let mut events = EventStream::new();
    let mut redraw_tick = tokio::time::interval(std::time::Duration::from_millis(200));
    let mut cron_tick = tokio::time::interval(runtime.cron_tick_interval());
    let mut runtime_tick = tokio::time::interval(runtime.runtime_tick_interval());

    loop {
        {
            let st = state.borrow();
            term.borrow_mut()
                .draw(|frame| view::render(frame, &st))
                .map_err(io_to_channel)?;
        }

        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(CrosstermEvent::Key(key))) => {
                        let Some(event) = map_key_event(key) else {
                            continue;
                        };
                        if handle_app_event::<ratatui::backend::CrosstermBackend<io::Stdout>>(
                            event,
                            &state,
                            &term,
                            runtime,
                        )
                        .await?
                        {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        state.borrow_mut().push_error(format!("event error: {err}"));
                    }
                    None => break,
                }
            }
            _ = redraw_tick.tick() => {
                let st = state.borrow();
                if st.status().is_submitting() || st.messages().is_empty() {
                    term
                        .borrow_mut()
                        .draw(|frame| view::render(frame, &st))
                        .map_err(io_to_channel)?;
                }
            }
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

fn io_to_channel(err: io::Error) -> Box<dyn std::error::Error> {
    Box::new(err)
}

async fn handle_app_event<B: Backend>(
    event: AppEvent,
    state: &Rc<RefCell<AppState>>,
    term: &Rc<RefCell<Terminal<B>>>,
    runtime: &dyn ChannelRuntime,
) -> ChannelResult<bool> {
    match event {
        AppEvent::Quit => Ok(true),
        AppEvent::ClearMessages => {
            state.borrow_mut().clear_messages();
            Ok(false)
        }
        AppEvent::NewSession => {
            state.borrow_mut().reset_session();
            Ok(false)
        }
        AppEvent::ToggleReasoning => {
            state.borrow_mut().toggle_reasoning();
            Ok(false)
        }
        AppEvent::Backspace => {
            state.borrow_mut().backspace();
            Ok(false)
        }
        AppEvent::Newline => {
            state.borrow_mut().insert_newline();
            Ok(false)
        }
        AppEvent::Input(ch) => {
            state.borrow_mut().insert_char(ch);
            Ok(false)
        }
        AppEvent::Submit => {
            let input = {
                let mut st = state.borrow_mut();
                let Some(submitted) = st.take_submit_input() else {
                    return Ok(false);
                };
                st.begin_agent_response();
                submitted
            };
            {
                let st = state.borrow();
                term.borrow_mut()
                    .draw(|frame| view::render(frame, &st))
                    .map_err(io_to_channel)?;
            }
            let request = {
                let st = state.borrow();
                ChannelRequest {
                    channel: st.meta().channel.clone(),
                    input,
                    session_key: st.meta().session_key.clone(),
                    chat_id: st.chat_id(),
                    media_references: Vec::new(),
                    metadata: BTreeMap::new(),
                }
            };
            let mut writer = InlineStreamWriter::<B> {
                state: Rc::clone(state),
                term: Rc::clone(term),
            };
            let maybe_output = runtime.submit_streaming(request, &mut writer).await?;
            match maybe_output {
                Some(output) => {
                    let show_reasoning = state.borrow().meta().show_reasoning;
                    let rendered =
                        render_agent_output(&output, show_reasoning, OutputRenderStyle::Terminal);
                    let mut st = state.borrow_mut();
                    st.apply_agent_snapshot(rendered);
                    st.complete_agent_response();
                }
                None => {
                    let mut st = state.borrow_mut();
                    st.clear_pending_agent_response();
                    st.push_error("[no response]");
                }
            }
            Ok(false)
        }
    }
}

struct InlineStreamWriter<B: Backend> {
    state: Rc<RefCell<AppState>>,
    term: Rc<RefCell<Terminal<B>>>,
}

#[async_trait::async_trait(?Send)]
impl<B: Backend> ChannelStreamWriter for InlineStreamWriter<B> {
    async fn write(&mut self, event: ChannelStreamEvent) -> ChannelResult<()> {
        let show_reasoning = self.state.borrow().meta().show_reasoning;
        match event {
            ChannelStreamEvent::Snapshot(output) => {
                let rendered =
                    render_agent_output(&output, show_reasoning, OutputRenderStyle::Terminal);
                self.state.borrow_mut().apply_agent_snapshot(rendered);
            }
            ChannelStreamEvent::Clear => {
                self.state.borrow_mut().apply_agent_snapshot(String::new());
            }
        }
        {
            let st = self.state.borrow();
            self.term
                .borrow_mut()
                .draw(|frame| view::render(frame, &st))
                .map_err(io_to_channel)?;
        }
        Ok(())
    }
}

fn setup_terminal() -> io::Result<Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
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
    use ratatui::{Terminal, backend::TestBackend};
    use std::{cell::RefCell, collections::BTreeMap, rc::Rc, time::Duration};

    #[derive(Default)]
    struct FakeRuntime {
        streaming_called: std::cell::Cell<bool>,
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
        let state = Rc::new(RefCell::new(AppState::new(sample_meta())));
        state.borrow_mut().set_input("hello".to_string());
        let backend = TestBackend::new(80, 24);
        let term = Rc::new(RefCell::new(Terminal::new(backend).expect("test terminal")));

        let should_quit =
            handle_app_event::<TestBackend>(AppEvent::Submit, &state, &term, &runtime)
                .await
                .expect("submit should succeed");

        assert!(!should_quit);
        assert!(runtime.streaming_called.get());
        assert_eq!(state.borrow().messages().len(), 2);
        assert!(state.borrow().messages()[1].content.contains("done"));
    }
}
