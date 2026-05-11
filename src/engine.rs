use std::path::Path;
use std::sync::mpsc::Sender;

use chrono::{DateTime, Utc};

use crate::chatbot_ctl;
use crate::config::{self, AppConfig, TargetConfig};
use crate::heuristics;
use crate::session_store::{self, SessionMessage};

#[derive(Clone, Debug)]
pub enum EngineAction {
    CheckAll,
    CheckTarget(usize),
    ForceSend(usize),
}

#[derive(Clone, Debug)]
pub enum TargetDecision {
    Sent,
    Skipped(String),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct TargetUpdate {
    pub index: usize,
    pub session_id: Option<String>,
    pub worker_running: Option<bool>,
    pub last_message_role: Option<String>,
    pub last_message_preview: Option<String>,
    pub build_docs_completed: Option<bool>,
    pub verification_session_id: Option<String>,
    pub target_override: Option<TargetConfig>,
    pub decision: TargetDecision,
    pub checked_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub enum EngineEvent {
    Log(String),
    TargetUpdated(TargetUpdate),
    Finished,
}

struct EngineDeps {
    session_id_for_name: fn(&Path, &str) -> anyhow::Result<Option<String>>,
    worker_running_for_chat: fn(&Path, &str) -> anyhow::Result<bool>,
    load_last_message: fn(&Path) -> anyhow::Result<Option<SessionMessage>>,
    send_continue: fn(&Path, &str, &str, &str) -> anyhow::Result<()>,
    create_chat: fn(&Path, &str, &str) -> anyhow::Result<String>,
    rename_chat: fn(&Path, &str, &str) -> anyhow::Result<()>,
    kill_chat: fn(&Path, &str) -> anyhow::Result<()>,
}

const NEXT_STEPS_OR_COMPLETION_PROMPT: &str = "Reply with one of the following only:\n\n1. An explicit \"Next Steps:\" section if more work remains.\n2. Exactly \"Design Doc Fully Implemented\" if the current iteration is complete.";

pub fn run(
    action: EngineAction,
    config: AppConfig,
    last_sent_at: Vec<Option<DateTime<Utc>>>,
    sender: Sender<EngineEvent>,
) {
    let indices: Vec<usize> = match action {
        EngineAction::CheckAll => config
            .targets
            .iter()
            .enumerate()
            .filter_map(|(index, target)| target.enabled.then_some(index))
            .collect(),
        EngineAction::CheckTarget(index) | EngineAction::ForceSend(index) => vec![index],
    };

    for index in indices {
        let event = process_target(index, &config, &last_sent_at, &action);
        let log_line = match config.targets.get(index) {
            Some(target) => format_log_line(target, &event),
            None => format!(
                "{} target[{index}] error: target index out of range",
                event.checked_at.to_rfc3339()
            ),
        };
        let _ = sender.send(EngineEvent::Log(log_line));
        let _ = sender.send(EngineEvent::TargetUpdated(event));
    }

    let _ = sender.send(EngineEvent::Finished);
}

fn process_target(
    index: usize,
    config: &AppConfig,
    last_sent_at: &[Option<DateTime<Utc>>],
    action: &EngineAction,
) -> TargetUpdate {
    let deps = EngineDeps {
        session_id_for_name: session_store::session_id_for_name,
        worker_running_for_chat: chatbot_ctl::worker_running_for_chat,
        load_last_message: session_store::load_last_message,
        send_continue: chatbot_ctl::send_continue,
        create_chat: chatbot_ctl::create_chat,
        rename_chat: chatbot_ctl::rename_chat,
        kill_chat: chatbot_ctl::kill_chat,
    };
    process_target_with_deps(index, config, last_sent_at, action, &deps)
}

fn process_target_with_deps(
    index: usize,
    config: &AppConfig,
    last_sent_at: &[Option<DateTime<Utc>>],
    action: &EngineAction,
    deps: &EngineDeps,
) -> TargetUpdate {
    let checked_at = Utc::now();
    let Some(target) = config.targets.get(index) else {
        return TargetUpdate {
            index,
            session_id: None,
            worker_running: None,
            last_message_role: None,
            last_message_preview: None,
            build_docs_completed: None,
            verification_session_id: None,
            target_override: None,
            decision: TargetDecision::Error("target index out of range".to_owned()),
            checked_at,
        };
    };

    let sessions_json = target.data_dir.join("sessions.json");
    let sessions_dir = target.data_dir.join("sessions");
    let session_id = match (deps.session_id_for_name)(&sessions_json, &target.name) {
        Ok(Some(session_id)) => session_id,
        Ok(None) => {
            return TargetUpdate {
                index,
                session_id: None,
                worker_running: None,
                last_message_role: None,
                last_message_preview: None,
                build_docs_completed: None,
                verification_session_id: None,
                target_override: None,
                decision: TargetDecision::Skipped("no visible session found".to_owned()),
                checked_at,
            };
        }
        Err(error) => {
            return TargetUpdate {
                index,
                session_id: None,
                worker_running: None,
                last_message_role: None,
                last_message_preview: None,
                build_docs_completed: None,
                verification_session_id: None,
                target_override: None,
                decision: TargetDecision::Error(error.to_string()),
                checked_at,
            };
        }
    };

    let worker_running = match (deps.worker_running_for_chat)(&config.chatbot_ctl_path, &session_id)
    {
        Ok(value) => value,
        Err(error) => {
            return TargetUpdate {
                index,
                session_id: Some(session_id),
                worker_running: None,
                last_message_role: None,
                last_message_preview: None,
                build_docs_completed: None,
                verification_session_id: None,
                target_override: None,
                decision: TargetDecision::Error(error.to_string()),
                checked_at,
            };
        }
    };

    if worker_running {
        return TargetUpdate {
            index,
            session_id: Some(session_id),
            worker_running: Some(true),
            last_message_role: None,
            last_message_preview: None,
            build_docs_completed: None,
            verification_session_id: None,
            target_override: None,
            decision: TargetDecision::Skipped("worker is already running".to_owned()),
            checked_at,
        };
    }

    let session_path = sessions_dir.join(format!("{session_id}.json"));
    let last_message = match (deps.load_last_message)(&session_path) {
        Ok(message) => message,
        Err(error) => {
            return TargetUpdate {
                index,
                session_id: Some(session_id),
                worker_running: Some(false),
                last_message_role: None,
                last_message_preview: None,
                build_docs_completed: None,
                verification_session_id: None,
                target_override: None,
                decision: TargetDecision::Error(error.to_string()),
                checked_at,
            };
        }
    };

    let message = match last_message {
        Some(message) => message,
        None => {
            return TargetUpdate {
                index,
                session_id: Some(session_id),
                worker_running: Some(false),
                last_message_role: None,
                last_message_preview: None,
                build_docs_completed: None,
                verification_session_id: None,
                target_override: None,
                decision: TargetDecision::Skipped("session has no messages".to_owned()),
                checked_at,
            };
        }
    };

    if message.is_user {
        return update_with_message(
            index,
            session_id,
            checked_at,
            &message,
            Some(false),
            None,
            None,
            None,
            TargetDecision::Skipped("latest message is from the user".to_owned()),
        );
    }

    if target.build_docs_before_continue && !target.build_docs_completed {
        let validation_issues = config::validate_bootstrap_target(target);
        if let Some(issue) = validation_issues.into_iter().next() {
            return update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                None,
                None,
                None,
                TargetDecision::Skipped(format!("build-doc bootstrap is enabled, but {issue}")),
            );
        }

        return match (deps.send_continue)(
            &config.chatbot_ctl_path,
            &target.provider,
            &session_id,
            &config::build_docs_prompt(&config.default_build_docs_prompt, target),
        ) {
            Ok(()) => update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                Some(true),
                None,
                None,
                TargetDecision::Sent,
            ),
            Err(error) => update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                None,
                None,
                None,
                TargetDecision::Error(error.to_string()),
            ),
        };
    }

    let assistant_reported_completion = heuristics::content_indicates_completion(&message.content);
    let assistant_provided_next_steps = heuristics::content_mentions_next_step(&message.content);

    if target.build_docs_before_continue
        && target.auto_increment_iterations
        && assistant_reported_completion
    {
        if target.current_iteration >= target.max_iterations {
            return update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                Some(target.build_docs_completed),
                None,
                None,
                TargetDecision::Skipped("completed and max iterations reached".to_owned()),
            );
        }

        let mut next_target = target.clone();
        next_target.build_docs_completed = false;
        config::advance_iteration(&mut next_target);
        let validation_issues = config::validate_bootstrap_target(&next_target);
        if let Some(issue) = validation_issues.into_iter().next() {
            return update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                Some(target.build_docs_completed),
                None,
                None,
                TargetDecision::Skipped(format!(
                    "auto-increment is enabled, but next iteration is invalid: {issue}"
                )),
            );
        }

        return match (deps.send_continue)(
            &config.chatbot_ctl_path,
            &next_target.provider,
            &session_id,
            &config::build_docs_prompt(&config.default_build_docs_prompt, &next_target),
        ) {
            Ok(()) => {
                next_target.build_docs_completed = true;
                update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    Some(true),
                    None,
                    Some(next_target),
                    TargetDecision::Sent,
                )
            }
            Err(error) => update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                Some(target.build_docs_completed),
                None,
                None,
                TargetDecision::Error(error.to_string()),
            ),
        };
    }

    if matches!(action, EngineAction::ForceSend(_)) {
        return match (deps.send_continue)(
            &config.chatbot_ctl_path,
            &target.provider,
            &session_id,
            &config::build_continue_text(&config.default_continue_text, target),
        ) {
            Ok(()) => update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                None,
                None,
                None,
                TargetDecision::Sent,
            ),
            Err(error) => update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                None,
                None,
                None,
                TargetDecision::Error(error.to_string()),
            ),
        };
    }

    if !assistant_provided_next_steps {
        if !assistant_reported_completion {
            if let Some(last_sent_at) = last_sent_at.get(index).and_then(|value| *value) {
                let elapsed = checked_at.signed_duration_since(last_sent_at).num_seconds();
                if elapsed >= 0 && elapsed < target.cooldown_seconds as i64 {
                    return update_with_message(
                        index,
                        session_id,
                        checked_at,
                        &message,
                        Some(false),
                        None,
                        None,
                        None,
                        TargetDecision::Skipped(format!(
                            "cooldown active before clarification send: {}s remaining",
                            target.cooldown_seconds.saturating_sub(elapsed as u64)
                        )),
                    );
                }
            }

            return match (deps.send_continue)(
                &config.chatbot_ctl_path,
                &target.provider,
                &session_id,
                NEXT_STEPS_OR_COMPLETION_PROMPT,
            ) {
                Ok(()) => update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    None,
                    None,
                    None,
                    TargetDecision::Sent,
                ),
                Err(error) => update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    None,
                    None,
                    None,
                    TargetDecision::Error(error.to_string()),
                ),
            };
        }

        return update_with_message(
            index,
            session_id,
            checked_at,
            &message,
            Some(false),
            None,
            None,
            None,
            TargetDecision::Skipped(
                "latest assistant message does not clearly request a next step".to_owned(),
            ),
        );
    }

    if let Some(last_sent_at) = last_sent_at.get(index).and_then(|value| *value) {
        let elapsed = checked_at.signed_duration_since(last_sent_at).num_seconds();
        if elapsed >= 0 && elapsed < target.cooldown_seconds as i64 {
            return update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                None,
                None,
                None,
                TargetDecision::Skipped(format!(
                    "cooldown active: {}s remaining",
                    target.cooldown_seconds.saturating_sub(elapsed as u64)
                )),
            );
        }
    }

    // Check if we're awaiting verification in a separate session
    if target.awaiting_verification {
        let Some(verification_id) = &target.verification_session_id else {
            // No verification session but awaiting flag is set — clear it
            let mut next_target = target.clone();
            next_target.awaiting_verification = false;
            next_target.verification_session_id = None;
            return update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                None,
                None,
                Some(next_target),
                TargetDecision::Skipped("verification session missing".to_owned()),
            );
        };

        // Check for timeout — 60 minute hard cap on verification session
        if let Some(started) = target.verification_started_at {
            let elapsed = checked_at.signed_duration_since(started).num_seconds();
            if elapsed >= 3600 {
                // Kill the timed-out verification session
                let _ = (deps.kill_chat)(&config.chatbot_ctl_path, verification_id);

                // Re-send the verification prompt to a new session
                let new_verification_name = format!("{} - VERIFY", target.name);
                let new_verification_id = match (deps.create_chat)(
                    &config.chatbot_ctl_path,
                    &target.provider,
                    &new_verification_name,
                ) {
                    Ok(id) => id,
                    Err(error) => {
                        let mut next_target = target.clone();
                        next_target.awaiting_verification = false;
                        next_target.verification_session_id = None;
                        next_target.verification_started_at = None;
                        return update_with_message(
                            index,
                            session_id,
                            checked_at,
                            &message,
                            Some(false),
                            Some(target.build_docs_completed),
                            None,
                            Some(next_target),
                            TargetDecision::Error(format!(
                                "verification timeout, failed to create new session: {error}"
                            )),
                        );
                    }
                };

                let verification_prompt = "Use sequential thinking -- fully assess the codebase and relay if the design doc was fully implemented. Emit a \"Next Steps:\" section if the design was not completed.";
                match (deps.send_continue)(
                    &config.chatbot_ctl_path,
                    &target.provider,
                    &new_verification_id,
                    verification_prompt,
                ) {
                    Ok(()) => {
                        let mut next_target = target.clone();
                        next_target.verification_session_id = Some(new_verification_id);
                        next_target.verification_started_at = Some(checked_at);
                        return update_with_message(
                            index,
                            session_id,
                            checked_at,
                            &message,
                            Some(false),
                            None,
                            None,
                            Some(next_target),
                            TargetDecision::Sent,
                        );
                    }
                    Err(error) => {
                        let mut next_target = target.clone();
                        next_target.awaiting_verification = false;
                        next_target.verification_session_id = None;
                        next_target.verification_started_at = None;
                        return update_with_message(
                            index,
                            session_id,
                            checked_at,
                            &message,
                            Some(false),
                            Some(target.build_docs_completed),
                            None,
                            Some(next_target),
                            TargetDecision::Error(format!(
                                "verification timeout, failed to re-send: {error}"
                            )),
                        );
                    }
                };
            }
        }

        // Load last message from verification session
        let verification_path = sessions_dir.join(format!("{verification_id}.json"));
        let verification_message = match (deps.load_last_message)(&verification_path) {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                return update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    None,
                    None,
                    None,
                    TargetDecision::Skipped("verification session has no messages yet".to_owned()),
                );
            }
            Err(error) => {
                return update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    None,
                    None,
                    None,
                    TargetDecision::Error(format!("failed to load verification message: {error}")),
                );
            }
        };

        let verif_completion =
            heuristics::content_indicates_completion(&verification_message.content);
        let verif_next_steps =
            heuristics::content_mentions_next_step(&verification_message.content);

        // Rename the main session based on verification result
        let main_name = format!(
            "{} - {}",
            target.name,
            if verif_completion && !verif_next_steps {
                "PASS"
            } else if verif_next_steps || verif_completion {
                "FAIL"
            } else {
                "CHECK"
            }
        );
        let _ = (deps.rename_chat)(&config.chatbot_ctl_path, &session_id, &main_name);

        // Also rename verification session
        let verif_name = format!("{} - VERIFY", target.name,);
        let _ = (deps.rename_chat)(&config.chatbot_ctl_path, verification_id, &verif_name);

        if verif_next_steps || (verif_completion && !verif_next_steps) {
            // Verification failed — assistant reported more work needed
            if target.current_iteration >= target.max_iterations {
                let mut next_target = target.clone();
                next_target.awaiting_verification = false;
                next_target.verification_session_id = None;
                return update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    None,
                    None,
                    Some(next_target),
                    TargetDecision::Skipped(
                        "verification failed and max iterations reached".to_owned(),
                    ),
                );
            }

            let mut next_target = target.clone();
            next_target.awaiting_verification = false;
            next_target.verification_session_id = None;
            next_target.build_docs_completed = false;
            config::advance_iteration(&mut next_target);
            let validation_issues = config::validate_bootstrap_target(&next_target);
            if let Some(issue) = validation_issues.into_iter().next() {
                return update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    None,
                    None,
                    Some(next_target),
                    TargetDecision::Skipped(format!(
                        "verification failed but next iteration invalid: {issue}"
                    )),
                );
            }

            // Send next iteration bootstrap
            return match (deps.send_continue)(
                &config.chatbot_ctl_path,
                &next_target.provider,
                &session_id,
                &config::build_docs_prompt(&config.default_build_docs_prompt, &next_target),
            ) {
                Ok(()) => {
                    let mut final_target = next_target;
                    final_target.build_docs_completed = true;
                    update_with_message(
                        index,
                        session_id,
                        checked_at,
                        &message,
                        Some(false),
                        Some(true),
                        None,
                        Some(final_target),
                        TargetDecision::Sent,
                    )
                }
                Err(error) => update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    Some(target.build_docs_completed),
                    None,
                    None,
                    TargetDecision::Error(error.to_string()),
                ),
            };
        }

        // Verification passed — design was fully implemented
        let mut next_target = target.clone();
        next_target.awaiting_verification = false;
        next_target.verification_session_id = None;
        return update_with_message(
            index,
            session_id,
            checked_at,
            &message,
            Some(false),
            None,
            None,
            Some(next_target),
            TargetDecision::Skipped("verification passed — design fully implemented".to_owned()),
        );
    }

    // Not awaiting verification — normal flow
    if target.build_docs_before_continue
        && target.auto_increment_iterations
        && assistant_reported_completion
    {
        // Create a new verification session
        let verification_name = format!("{} - VERIFY", target.name);
        let verification_id = match (deps.create_chat)(
            &config.chatbot_ctl_path,
            &target.provider,
            &verification_name,
        ) {
            Ok(id) => id,
            Err(error) => {
                return update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    Some(target.build_docs_completed),
                    None,
                    None,
                    TargetDecision::Error(format!(
                        "failed to create verification session: {error}"
                    )),
                );
            }
        };

        let verification_prompt = "Use sequential thinking -- fully assess the codebase and relay if the design doc was fully implemented. Emit a \"Next Steps:\" section if the design was not completed.";

        return match (deps.send_continue)(
            &config.chatbot_ctl_path,
            &target.provider,
            &verification_id,
            verification_prompt,
        ) {
            Ok(()) => {
                let mut next_target = target.clone();
                next_target.awaiting_verification = true;
                next_target.verification_session_id = Some(verification_id);
                next_target.verification_started_at = Some(checked_at);
                update_with_message(
                    index,
                    session_id,
                    checked_at,
                    &message,
                    Some(false),
                    None,
                    None,
                    Some(next_target),
                    TargetDecision::Sent,
                )
            }
            Err(error) => update_with_message(
                index,
                session_id,
                checked_at,
                &message,
                Some(false),
                Some(target.build_docs_completed),
                None,
                None,
                TargetDecision::Error(error.to_string()),
            ),
        };
    }

    // Normal case: assistant has next steps, send continue text
    return match (deps.send_continue)(
        &config.chatbot_ctl_path,
        &target.provider,
        &session_id,
        &config::build_continue_text(&config.default_continue_text, target),
    ) {
        Ok(()) => update_with_message(
            index,
            session_id,
            checked_at,
            &message,
            Some(false),
            None,
            None,
            None,
            TargetDecision::Sent,
        ),
        Err(error) => update_with_message(
            index,
            session_id,
            checked_at,
            &message,
            Some(false),
            None,
            None,
            None,
            TargetDecision::Error(error.to_string()),
        ),
    };
}

fn update_with_message(
    index: usize,
    session_id: String,
    checked_at: DateTime<Utc>,
    message: &SessionMessage,
    worker_running: Option<bool>,
    build_docs_completed: Option<bool>,
    verification_session_id: Option<String>,
    target_override: Option<TargetConfig>,
    decision: TargetDecision,
) -> TargetUpdate {
    TargetUpdate {
        index,
        session_id: Some(session_id),
        worker_running,
        last_message_role: Some(if message.is_user {
            "user".to_owned()
        } else {
            "assistant".to_owned()
        }),
        last_message_preview: Some(truncate_preview(&message.content)),
        build_docs_completed,
        verification_session_id,
        target_override,
        decision,
        checked_at,
    }
}

fn truncate_preview(content: &str) -> String {
    const LIMIT: usize = 140;
    let flattened = content.replace('\n', " ");
    if flattened.chars().count() <= LIMIT {
        return flattened;
    }

    let truncated: String = flattened.chars().take(LIMIT).collect();
    format!("{truncated}...")
}

fn format_log_line(target: &TargetConfig, update: &TargetUpdate) -> String {
    let decision = match &update.decision {
        TargetDecision::Sent => "sent continue".to_owned(),
        TargetDecision::Skipped(reason) => format!("skipped: {reason}"),
        TargetDecision::Error(error) => format!("error: {error}"),
    };

    format!(
        "{} {}:{} {}",
        update.checked_at.to_rfc3339(),
        target.provider,
        target.name,
        decision
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_reports_invalid_target_actions_and_finishes_without_panicking() {
        for action in [
            EngineAction::CheckTarget(usize::MAX),
            EngineAction::ForceSend(usize::MAX),
        ] {
            let (sender, receiver) = std::sync::mpsc::channel();
            run(action, AppConfig::default(), vec![], sender);
            let events: Vec<_> = receiver.try_iter().collect();
            assert_eq!(events.len(), 3);
            assert!(
                matches!(&events[0], EngineEvent::Log(line) if line.contains("target index out of range"))
            );
            assert!(matches!(&events[1], EngineEvent::TargetUpdated(update)
                if update.index == usize::MAX
                    && matches!(&update.decision, TargetDecision::Error(message) if message == "target index out of range")));
            assert!(matches!(events[2], EngineEvent::Finished));
        }
    }

    use super::*;
    use anyhow::anyhow;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    #[derive(Clone, Debug)]
    struct EngineTestState {
        session_id: Option<String>,
        worker_running: bool,
        last_message: Option<SessionMessage>,
        send_error: Option<String>,
        sent_texts: Vec<String>,
    }

    fn state_cell() -> &'static Mutex<EngineTestState> {
        static CELL: OnceLock<Mutex<EngineTestState>> = OnceLock::new();
        CELL.get_or_init(|| {
            Mutex::new(EngineTestState {
                session_id: Some("chat-123".to_string()),
                worker_running: false,
                last_message: Some(SessionMessage {
                    is_user: false,
                    content: "Next Steps:\n1. Continue with repo setup".to_string(),
                }),
                send_error: None,
                sent_texts: Vec::new(),
            })
        })
    }

    fn test_guard() -> &'static Mutex<()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD.get_or_init(|| Mutex::new(()))
    }

    fn with_state<R>(f: impl FnOnce(&mut EngineTestState) -> R) -> R {
        let mut guard = state_cell()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut guard)
    }

    fn reset_state() {
        with_state(|state| {
            *state = EngineTestState {
                session_id: Some("chat-123".to_string()),
                worker_running: false,
                last_message: Some(SessionMessage {
                    is_user: false,
                    content: "Next Steps:\n1. Continue with repo setup".to_string(),
                }),
                send_error: None,
                sent_texts: Vec::new(),
            };
        });
    }

    fn fake_session_id_for_name(_path: &Path, _name: &str) -> anyhow::Result<Option<String>> {
        Ok(with_state(|state| state.session_id.clone()))
    }

    fn fake_worker_running_for_chat(_path: &Path, _chat_id: &str) -> anyhow::Result<bool> {
        Ok(with_state(|state| state.worker_running))
    }

    fn fake_load_last_message(_path: &Path) -> anyhow::Result<Option<SessionMessage>> {
        Ok(with_state(|state| state.last_message.clone()))
    }

    fn fake_send_continue(
        _path: &Path,
        _provider: &str,
        _chat_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        with_state(|state| {
            state.sent_texts.push(text.to_string());
            state.send_error.clone()
        })
        .map_or(Ok(()), |error| Err(anyhow!(error)))
    }

    fn fake_create_chat(_path: &Path, _provider: &str, _name: &str) -> anyhow::Result<String> {
        Ok("verify-chat-123".to_string())
    }

    fn fake_rename_chat(_path: &Path, _chat_id: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn fake_kill_chat(_path: &Path, _chat_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn test_deps() -> EngineDeps {
        EngineDeps {
            session_id_for_name: fake_session_id_for_name,
            worker_running_for_chat: fake_worker_running_for_chat,
            load_last_message: fake_load_last_message,
            send_continue: fake_send_continue,
            create_chat: fake_create_chat,
            rename_chat: fake_rename_chat,
            kill_chat: fake_kill_chat,
        }
    }

    fn bootstrap_target() -> TargetConfig {
        TargetConfig {
            name: "ziglua".to_string(),
            provider: "codex".to_string(),
            data_dir: PathBuf::from("/tmp/provider"),
            repo_path: PathBuf::from("/tmp/repo"),
            design_document: "/tmp/repo/docs/design.md".to_string(),
            task_document: "/tmp/repo/docs/task.md".to_string(),
            build_docs_before_continue: true,
            ..TargetConfig::default()
        }
    }

    fn bootstrap_config() -> AppConfig {
        AppConfig {
            chatbot_ctl_path: PathBuf::from("/tmp/chatbot-ctl"),
            default_build_docs_prompt: "bootstrap docs".to_string(),
            targets: vec![bootstrap_target()],
            ..AppConfig::default()
        }
    }

    #[test]
    fn bootstrap_check_sends_build_docs_prompt_first() {
        let _guard = test_guard()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_state();
        let config = bootstrap_config();

        let update =
            process_target_with_deps(0, &config, &[None], &EngineAction::CheckAll, &test_deps());

        assert!(matches!(update.decision, TargetDecision::Sent));
        assert_eq!(update.build_docs_completed, Some(true));
        let state = with_state(|state| state.clone());
        assert_eq!(state.sent_texts.len(), 1);
        assert_eq!(
            state.sent_texts[0],
            config::build_docs_prompt(&config.default_build_docs_prompt, &config.targets[0])
        );
    }

    #[test]
    fn bootstrap_check_skips_when_repo_path_is_missing() {
        let _guard = test_guard()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_state();
        let mut config = bootstrap_config();
        config.targets[0].repo_path = PathBuf::new();

        let update =
            process_target_with_deps(0, &config, &[None], &EngineAction::CheckAll, &test_deps());

        match update.decision {
            TargetDecision::Skipped(reason) => {
                assert!(reason.contains("repo path is empty"));
            }
            other => panic!("expected skip, got {other:?}"),
        }
        assert_eq!(update.build_docs_completed, None);
        assert!(with_state(|state| state.sent_texts.is_empty()));
    }

    #[test]
    fn bootstrap_check_skips_when_design_document_path_is_missing() {
        let _guard = test_guard()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_state();
        let mut config = bootstrap_config();
        config.targets[0].design_document.clear();

        let update =
            process_target_with_deps(0, &config, &[None], &EngineAction::CheckAll, &test_deps());

        match update.decision {
            TargetDecision::Skipped(reason) => {
                assert!(reason.contains("design document path is empty"));
            }
            other => panic!("expected skip, got {other:?}"),
        }
        assert_eq!(update.build_docs_completed, None);
        assert!(with_state(|state| state.sent_texts.is_empty()));
    }

    #[test]
    fn bootstrap_check_reports_send_errors() {
        let _guard = test_guard()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_state();
        with_state(|state| state.send_error = Some("send failed".to_string()));
        let config = bootstrap_config();

        let update =
            process_target_with_deps(0, &config, &[None], &EngineAction::CheckAll, &test_deps());

        match update.decision {
            TargetDecision::Error(error) => assert!(error.contains("send failed")),
            other => panic!("expected error, got {other:?}"),
        }
        assert_eq!(update.build_docs_completed, None);
        assert_eq!(with_state(|state| state.sent_texts.len()), 1);
    }

    #[test]
    fn completion_with_auto_increment_sends_next_iteration_prompt() {
        let _guard = test_guard()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_state();
        with_state(|state| {
            state.last_message = Some(SessionMessage {
                is_user: false,
                content: "All work is complete.".to_string(),
            });
        });
        let mut config = bootstrap_config();
        config.targets[0].build_docs_completed = true;
        config.targets[0].auto_increment_iterations = true;
        config.targets[0].current_iteration = 1;
        config.targets[0].max_iterations = 3;

        let update =
            process_target_with_deps(0, &config, &[None], &EngineAction::CheckAll, &test_deps());

        assert!(matches!(update.decision, TargetDecision::Sent));
        let next_target = update
            .target_override
            .expect("target override should carry next iteration state");
        assert_eq!(next_target.current_iteration, 2);
        assert_eq!(next_target.design_document, "/tmp/repo/docs/design-2.md");
        assert_eq!(next_target.task_document, "/tmp/repo/docs/task-2.md");
        let state = with_state(|state| state.clone());
        assert_eq!(state.sent_texts.len(), 1);
        assert!(state.sent_texts[0].contains("design-2.md"));
        assert!(state.sent_texts[0].contains("task-2.md"));
    }

    #[test]
    fn completion_with_auto_increment_stops_at_max_iterations() {
        let _guard = test_guard()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_state();
        with_state(|state| {
            state.last_message = Some(SessionMessage {
                is_user: false,
                content: "No more work remains.".to_string(),
            });
        });
        let mut config = bootstrap_config();
        config.targets[0].build_docs_completed = true;
        config.targets[0].auto_increment_iterations = true;
        config.targets[0].current_iteration = 2;
        config.targets[0].max_iterations = 2;

        let update =
            process_target_with_deps(0, &config, &[None], &EngineAction::CheckAll, &test_deps());

        match update.decision {
            TargetDecision::Skipped(reason) => {
                assert!(reason.contains("max iterations reached"));
            }
            other => panic!("expected skip, got {other:?}"),
        }
        assert!(update.target_override.is_none());
        assert!(with_state(|state| state.sent_texts.is_empty()));
    }

    #[test]
    fn ambiguous_assistant_reply_requests_next_steps_or_completion_marker() {
        let _guard = test_guard()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_state();
        with_state(|state| {
            state.last_message = Some(SessionMessage {
                is_user: false,
                content: "Implemented the requested changes and added tests.".to_string(),
            });
        });
        let mut config = bootstrap_config();
        config.targets[0].build_docs_before_continue = false;
        config.targets[0].build_docs_completed = true;

        let update =
            process_target_with_deps(0, &config, &[None], &EngineAction::CheckAll, &test_deps());

        assert!(matches!(update.decision, TargetDecision::Sent));
        let state = with_state(|state| state.clone());
        assert_eq!(state.sent_texts.len(), 1);
        assert_eq!(state.sent_texts[0], NEXT_STEPS_OR_COMPLETION_PROMPT);
    }

    #[test]
    fn emphasized_design_doc_no_still_counts_as_more_work() {
        let _guard = test_guard()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_state();
        with_state(|state| {
            state.last_message = Some(SessionMessage {
                is_user: false,
                content: "## Tasks Accomplished\n\
Completed PR 8.\n\
---\n\
## Next Steps:\n\
Task 210 remains blocked on zigmera-lowering.\n\
Design Doc Fully Implemented: **NO** - Tasks 210-211 remain."
                    .to_string(),
            });
        });
        let mut config = bootstrap_config();
        config.targets[0].build_docs_before_continue = false;
        config.targets[0].build_docs_completed = true;

        let update =
            process_target_with_deps(0, &config, &[None], &EngineAction::CheckAll, &test_deps());

        assert!(matches!(update.decision, TargetDecision::Sent));
        let state = with_state(|state| state.clone());
        assert_eq!(state.sent_texts.len(), 1);
        assert_eq!(
            state.sent_texts[0],
            config::build_continue_text(&config.default_continue_text, &config.targets[0])
        );
    }
}
