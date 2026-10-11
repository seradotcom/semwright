//! Human approval on the attached Windows console, with no approval channel over IPC.
use async_trait::async_trait;
use semwright_core::{Approval, Approver};
use semwright_platform_services::ConsoleTerminal;
use semwright_types::{Error, ErrorCode, Result, unique_id};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct Console;

fn escaped(text: &str) -> String {
    text.chars()
        .flat_map(char::escape_default)
        .take(8192)
        .collect()
}

impl Console {
    /// Refuse service/detached startup before advertising an unusable approval endpoint.
    pub fn validate() -> Result<()> {
        ConsoleTerminal::open().map(|_| ())
    }
}

struct InputGuard(ConsoleTerminal);
impl Drop for InputGuard {
    fn drop(&mut self) {
        let _ = self.0.flush();
    }
}

async fn read_answer(
    terminal: &ConsoleTerminal,
    cancellation: &CancellationToken,
) -> Result<String> {
    let mut answer = String::new();
    loop {
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Approval request cancelled before dispatch",
            ));
        }
        for key in terminal.read_keys()? {
            match key {
                '\r' | '\n' => {
                    terminal.write("\r\n")?;
                    return Ok(answer);
                }
                '\u{8}' if !answer.is_empty() => {
                    answer.pop();
                    terminal.write("\u{8} \u{8}")?;
                }
                '\u{8}' => {}
                ' '..='~' if answer.len() < 256 => {
                    answer.push(key);
                    terminal.write(&key.to_string())?;
                }
                _ => {
                    return Err(Error::new(
                        ErrorCode::PolicyDenied,
                        "Approval response exceeds the limit or contains control characters",
                    ));
                }
            }
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(Error::new(ErrorCode::Cancelled, "Approval request cancelled before dispatch")),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
    }
}

#[async_trait]
impl Approver for Console {
    async fn approve(&self, approval: Approval, cancellation: CancellationToken) -> Result<bool> {
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Approval request cancelled before dispatch",
            ));
        }
        let terminal = InputGuard(ConsoleTerminal::open()?);
        terminal.0.flush()?;
        let challenge = unique_id()[..12].to_owned();
        terminal.0.write(&format!(
            "\r\nSEMWRIGHT HUMAN APPROVAL\r\nCommand: {}\r\nBackend: {}\r\nRisk: {:?}\r\n{}\r\nBroker desktop I/O is locked while this question is pending.\r\nType exactly: approve {}\r\n> ",
            escaped(&approval.command), escaped(&approval.backend), approval.risk,
            escaped(&approval.summary), challenge,
        ))?;
        let answer = tokio::time::timeout(
            Duration::from_secs(120),
            read_answer(&terminal.0, &cancellation),
        )
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::ConsentRequired,
                "Approval expired before dispatch",
            )
        })??;
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Approval request cancelled before dispatch",
            ));
        }
        let granted = answer == format!("approve {challenge}");
        terminal.0.write(if granted {
            "Approved once.\r\n"
        } else {
            "Declined.\r\n"
        })?;
        Ok(granted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_text_cannot_rewrite_the_console_prompt() {
        assert_eq!(
            escaped("fake\x1b[2J\r\n approve"),
            "fake\\u{1b}[2J\\r\\n approve"
        );
        assert!(escaped(&"x".repeat(9000)).len() <= 8192);
    }
}
