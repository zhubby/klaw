use crate::{Channel, ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime};
use crossterm::{
    cursor::MoveToColumn,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};
use tokio::sync::mpsc;
use tracing::info;
use uuid::Uuid;

const PROMPT: &str = "you> ";
static TERMINAL_STATE: OnceLock<Arc<Mutex<TerminalState>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct StdioChannel {
    session_key: String,
    chat_id: String,
    show_reasoning: bool,
}

impl StdioChannel {
    pub fn new(session_key: Option<String>, show_reasoning: bool) -> Self {
        let session_key = session_key.unwrap_or_else(|| format!("stdio:{}", Uuid::new_v4()));
        let chat_id = session_key.split(':').nth(1).unwrap_or("chat").to_string();
        let _ = terminal_state();
        Self {
            session_key,
            chat_id,
            show_reasoning,
        }
    }

    pub fn session_key(&self) -> &str {
        &self.session_key
    }
}

#[async_trait::async_trait(?Send)]
impl Channel for StdioChannel {
    fn name(&self) -> &'static str {
        "stdio"
    }

    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
        let terminal = TerminalController::new(PROMPT);
        terminal.print_line("Type your message and press Enter.")?;
        terminal.print_line("Use /exit to quit.")?;
        terminal.print_line("")?;
        info!(session_key = %self.session_key, "stdio channel started");

        let mut input = InputReader::spawn(&terminal)?;
        let mut cron_tick = tokio::time::interval(runtime.cron_tick_interval());
        let mut runtime_tick = tokio::time::interval(runtime.runtime_tick_interval());

        terminal.show_prompt()?;

        loop {
            tokio::select! {
                _ = cron_tick.tick() => {
                    runtime.on_cron_tick().await;
                }
                _ = runtime_tick.tick() => {
                    runtime.on_runtime_tick().await;
                }
                event = input.recv() => {
                    let Some(event) = event else {
                        terminal.print_line("EOF received. Bye.")?;
                        break;
                    };

                    match event {
                        InputEvent::Interrupt => {
                            terminal.clear_prompt()?;
                            terminal.print_line("Ctrl+C received. Bye.")?;
                            break;
                        }
                        InputEvent::Line(line) => {
                            let submitted = line.trim().to_string();
                            if submitted.is_empty() {
                                terminal.show_prompt()?;
                                continue;
                            }

                            terminal.commit_input(&submitted)?;
                            if submitted == "/exit" {
                                terminal.print_line("Bye.")?;
                                break;
                            }

                            let maybe_output = runtime
                                .submit(ChannelRequest {
                                    channel: self.name().to_string(),
                                    input: submitted,
                                    session_key: self.session_key.clone(),
                                    chat_id: self.chat_id.clone(),
                                })
                                .await?;

                            match maybe_output {
                                Some(output) => terminal.print_line(&format!(
                                    "agent>\n{}\n",
                                    render_agent_output(&output, self.show_reasoning)
                                ))?,
                                None => terminal.print_line("agent> [no response]\n")?,
                            }
                            terminal.show_prompt()?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn make_tracing_writer() -> TerminalLogWriter {
    TerminalLogWriter::new(terminal_state())
}

fn terminal_state() -> Arc<Mutex<TerminalState>> {
    Arc::clone(TERMINAL_STATE.get_or_init(|| Arc::new(Mutex::new(TerminalState::default()))))
}

#[derive(Debug, Default)]
struct TerminalState {
    prompt: String,
    buffer: String,
    prompt_visible: bool,
}

#[derive(Debug, Clone)]
struct TerminalController {
    state: Arc<Mutex<TerminalState>>,
    prompt: String,
}

impl TerminalController {
    fn new(prompt: &str) -> Self {
        Self {
            state: terminal_state(),
            prompt: prompt.to_string(),
        }
    }

    fn show_prompt(&self) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        let mut state = self.lock_state();
        state.prompt = self.prompt.clone();
        state.buffer.clear();
        state.prompt_visible = true;
        redraw_prompt(&mut stdout, &state)?;
        stdout.flush()
    }

    fn update_buffer(&self, buffer: String) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        let mut state = self.lock_state();
        state.prompt = self.prompt.clone();
        state.buffer = buffer;
        state.prompt_visible = true;
        redraw_prompt(&mut stdout, &state)?;
        stdout.flush()
    }

    fn commit_input(&self, input: &str) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        let mut state = self.lock_state();
        state.prompt = self.prompt.clone();
        state.buffer.clear();
        state.prompt_visible = false;
        clear_prompt_line(&mut stdout)?;
        writeln!(stdout, "{}{}", self.prompt, input)?;
        stdout.flush()
    }

    fn clear_prompt(&self) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        let mut state = self.lock_state();
        state.buffer.clear();
        state.prompt_visible = false;
        clear_prompt_line(&mut stdout)?;
        stdout.flush()
    }

    fn print_line(&self, message: &str) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        let state = self.lock_state();
        print_with_prompt_restored(&mut stdout, &state, message)?;
        stdout.flush()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, TerminalState> {
        self.state.lock().unwrap_or_else(|err| err.into_inner())
    }
}

#[derive(Debug)]
enum InputEvent {
    Line(String),
    Interrupt,
}

#[derive(Debug)]
struct InputReader {
    receiver: mpsc::UnboundedReceiver<InputEvent>,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl InputReader {
    fn spawn(terminal: &TerminalController) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = Arc::clone(&running);
        let terminal = terminal.clone();
        let (sender, receiver) = mpsc::unbounded_channel();
        let handle = thread::spawn(move || {
            let mut buffer = String::new();
            while thread_running.load(Ordering::Relaxed) {
                match event::poll(Duration::from_millis(100)) {
                    Ok(true) => match event::read() {
                        Ok(Event::Key(key))
                            if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                        {
                            match key.code {
                                KeyCode::Char('c')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    let _ = sender.send(InputEvent::Interrupt);
                                    break;
                                }
                                KeyCode::Enter => {
                                    let line = std::mem::take(&mut buffer);
                                    let _ = terminal.update_buffer(String::new());
                                    if sender.send(InputEvent::Line(line)).is_err() {
                                        break;
                                    }
                                }
                                KeyCode::Backspace => {
                                    buffer.pop();
                                    let _ = terminal.update_buffer(buffer.clone());
                                }
                                KeyCode::Char(ch)
                                    if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    buffer.push(ch);
                                    let _ = terminal.update_buffer(buffer.clone());
                                }
                                KeyCode::Tab => {
                                    buffer.push('\t');
                                    let _ = terminal.update_buffer(buffer.clone());
                                }
                                _ => {}
                            }
                        }
                        Ok(Event::Paste(text)) => {
                            buffer.push_str(&text);
                            let _ = terminal.update_buffer(buffer.clone());
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    },
                    Ok(false) => {}
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            receiver,
            running,
            handle: Some(handle),
        })
    }

    async fn recv(&mut self) -> Option<InputEvent> {
        self.receiver.recv().await
    }
}

impl Drop for InputReader {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = terminal::disable_raw_mode();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerminalLogWriter {
    state: Arc<Mutex<TerminalState>>,
    buffer: Vec<u8>,
}

impl TerminalLogWriter {
    fn new(state: Arc<Mutex<TerminalState>>) -> Self {
        Self {
            state,
            buffer: Vec::new(),
        }
    }
}

impl Write for TerminalLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let mut stdout = io::stdout().lock();
        let state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        let message = String::from_utf8_lossy(&self.buffer);
        print_with_prompt_restored(&mut stdout, &state, &message)?;
        stdout.flush()?;
        self.buffer.clear();
        Ok(())
    }
}

fn print_with_prompt_restored(
    stdout: &mut impl Write,
    state: &TerminalState,
    message: &str,
) -> io::Result<()> {
    if state.prompt_visible {
        clear_prompt_line(stdout)?;
    }
    write!(stdout, "{message}")?;
    if !message.ends_with('\n') {
        writeln!(stdout)?;
    }
    if state.prompt_visible {
        redraw_prompt(stdout, state)?;
    }
    Ok(())
}

fn clear_prompt_line(stdout: &mut impl Write) -> io::Result<()> {
    execute!(stdout, MoveToColumn(0), Clear(ClearType::CurrentLine))
}

fn redraw_prompt(stdout: &mut impl Write, state: &TerminalState) -> io::Result<()> {
    queue!(
        stdout,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        Print(format!("{}{}", state.prompt, state.buffer)),
    )?;
    Ok(())
}

fn render_agent_output(output: &ChannelResponse, show_reasoning: bool) -> String {
    let mut lines = vec![
        "--------------------".to_string(),
        "[answer]".to_string(),
        output.content.trim().to_string(),
    ];
    if show_reasoning {
        if let Some(reasoning_text) = &output.reasoning {
            lines.push(String::new());
            lines.push("[reasoning]".to_string());
            lines.extend(reasoning_text.lines().map(|line| format!("> {line}")));
        }
    }
    lines.push("--------------------".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{render_agent_output, StdioChannel, TerminalState};
    use crate::ChannelResponse;

    #[test]
    fn keeps_explicit_session_key() {
        let channel = StdioChannel::new(Some("stdio:explicit".to_string()), false);
        assert_eq!(channel.session_key(), "stdio:explicit");
    }

    #[test]
    fn hides_reasoning_when_flag_disabled() {
        let view = render_agent_output(
            &ChannelResponse {
                content: "done".to_string(),
                reasoning: Some("step1\nstep2".to_string()),
            },
            false,
        );
        assert!(view.contains("[answer]"));
        assert!(!view.contains("[reasoning]"));
    }

    #[test]
    fn renders_reasoning_block_when_enabled() {
        let view = render_agent_output(
            &ChannelResponse {
                content: "done".to_string(),
                reasoning: Some("step1\nstep2".to_string()),
            },
            true,
        );
        assert!(view.contains("[reasoning]"));
        assert!(view.contains("> step1"));
        assert!(view.contains("> step2"));
    }

    #[test]
    fn terminal_state_defaults_to_hidden_prompt() {
        let state = TerminalState::default();
        assert!(state.prompt.is_empty());
        assert!(state.buffer.is_empty());
        assert!(!state.prompt_visible);
    }
}
