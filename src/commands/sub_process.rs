use crate::error::AppResult;
use crate::types::state::AppState;
use crate::ui::AppBackend;
use std::path::Path;
use std::process::{Command, Stdio};

use super::reload;

#[derive(Debug, Clone)]
pub enum SubprocessCallMode {
    Interactive,
    Spawn,
    Capture,
}

pub fn current_files(app_state: &AppState) -> Vec<(&str, &Path)> {
    let mut result = Vec::new();
    if let Some(curr_list) = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .curr_list_ref()
    {
        let mut i = 0;
        curr_list.iter_selected().for_each(|entry| {
            result.push((entry.file_name(), entry.file_path()));
            i += 1;
        });
        if i == 0 {
            if let Some(entry) = curr_list.curr_entry_ref() {
                result.push((entry.file_name(), entry.file_path()));
            }
        }
    }

    result
}

fn current_dir(app_state: &AppState) -> &Path {
    app_state.state.tab_state_ref().curr_tab_ref().get_cwd()
}

fn execute_sub_process(
    app_state: &mut AppState,
    words: &[String],
    mode: SubprocessCallMode,
) -> std::io::Result<()> {
    let current_files = current_files(app_state);

    let mut command: Command;
    if let SubprocessCallMode::Interactive = mode {
        let mut cmd = String::new();
        command = Command::new("sh");
        command.arg("-c");
        if current_files.len() == 1 {
            let (file_name, file_path) = current_files[0];
            cmd.push_str(
                words[0]
                    .replace("%s", file_name)
                    .replace("%p", &file_path.to_string_lossy())
                    .as_str(),
            );
        } else {
            cmd.push_str(words[0].as_str());
        }

        for word in words.iter().skip(1) {
            cmd.push(' ');
            match word.as_str() {
                "%s" => {
                    if let Some((start, _)) = current_files.get(0) {
                        cmd.push_str(
                            &current_files
                                .iter()
                                .skip(1)
                                .map(|(file_name, _)| file_name)
                                .fold(start.to_string(), |acc, e| {
                                    format!("{acc} \"{e}\"")
                                }),
                        );
                    }

                    // for (i, (file_name, _)) in current_files.iter().enumerate() {
                    //     cmd.push_str(&format!("\"{}\"", file_name));
                    //     if i < current_files.len() - 1 {
                    //         cmd.push(' ');
                    //     }
                    // }
                }
                "%p" => {
                    if let Some((_, start)) = current_files.get(0) {
                        cmd.push_str(
                            &current_files
                                .iter()
                                .skip(1)
                                .map(|(_, file_path)| file_path.to_string_lossy())
                                .fold(start.to_string_lossy().to_string(), |acc, e| {
                                    format!("{acc} \"{e}\"")
                                }),
                        );
                    }

                    // for (i, (_, file_path)) in current_files.iter().enumerate() {
                    //     cmd.push_str(&format!("\"{}\"", file_path.to_string_lossy()));
                    //     if i < current_files.len() - 1 {
                    //         cmd.push(' ');
                    //     }
                    // }
                }
                "%d" => {
                    cmd.push_str(&current_dir(app_state).to_string_lossy());
                }
                s => {
                    cmd.push_str(s);
                }
            };
        }

        command.arg(cmd);
    } else {
        let command_base = if current_files.len() == 1 {
            let (file_name, file_path) = current_files[0];

            words[0]
                .replace("%s", file_name)
                .replace("%p", &file_path.to_string_lossy())
        } else {
            words[0].clone()
        };

        command = Command::new(command_base);
        for word in words.iter().skip(1) {
            match word.as_str() {
                "%s" => {
                    for (file_name, _) in &current_files {
                        command.arg(file_name);
                    }
                }
                "%p" => {
                    for (_, file_path) in &current_files {
                        command.arg(file_path);
                    }
                }
                "%d" => {
                    command.arg(current_dir(app_state));
                }
                s => {
                    command.arg(s);
                }
            };
        }
    }

    match mode {
        SubprocessCallMode::Interactive => {
            let status = command.status();
            match status {
                Ok(status) => {
                    if status.code() == Some(0) {
                        Ok(())
                    } else {
                        Err(std::io::Error::other(format!(
                            "Command failed with {}",
                            status
                        )))
                    }
                }
                Err(err) => Err(std::io::Error::other(format!(
                    "Shell execution failed: {}",
                    err
                ))),
            }
        }
        SubprocessCallMode::Spawn => {
            command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            Ok(())
        }
        SubprocessCallMode::Capture => {
            let output = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()?;
            if output.status.code() == Some(0) {
                app_state.state.last_stdout =
                    Some(String::from_utf8_lossy(&output.stdout).to_string());
                Ok(())
            } else {
                Err(std::io::Error::other(format!(
                    "Command failed with {}",
                    output.status
                )))
            }
        }
    }
}

/// Handler for Joshuto's `shell` and `spawn` commands.
pub fn sub_process(
    app_state: &mut AppState,
    backend: &mut AppBackend,
    words: &[String],
    mode: SubprocessCallMode,
) -> AppResult {
    match mode {
        SubprocessCallMode::Interactive => {
            // Joshuto needs to release the terminal when handing it over to some interactive
            // shell command and restore it afterwards
            backend.terminal_drop();
            let res = execute_sub_process(app_state, words, mode);
            backend.terminal_restore()?;
            let _ = reload::soft_reload_curr_tab(app_state);
            res?;
            app_state
                .state
                .message_queue_mut()
                .push_info(format!("Finished: {}", words.join(" ")));
        }
        SubprocessCallMode::Spawn => {
            execute_sub_process(app_state, words, mode)?;
            app_state
                .state
                .message_queue_mut()
                .push_info(format!("Spawned: {}", words.join(" ")));
        }
        SubprocessCallMode::Capture => {
            execute_sub_process(app_state, words, mode)?;
            let _ = reload::soft_reload_curr_tab(app_state);
        }
    };
    Ok(())
}
