use super::cancel::Cancellation;
use super::client::CompanionClient;
use super::pack::{pack_messages, NoopRecall, RecallInjector};
use super::tools::{parse_tool_object, NoopExecutor, ToolExecutor};
use super::types::{Finish, Message};
use super::CompanionError;
use crate::text;

const MAX_TOOL_ROUNDS: usize = 8;

pub struct Session<E = NoopExecutor, R = NoopRecall> {
    client: CompanionClient,
    history: Vec<Message>,
    window: u32,
    executor: E,
    recall: R,
}

impl Session<NoopExecutor, NoopRecall> {
    pub fn new(client: CompanionClient, window: u32) -> Self {
        Self {
            client,
            history: vec![Message::system(text::get("companion.system_prompt"))],
            window,
            executor: NoopExecutor,
            recall: NoopRecall,
        }
    }
}

impl<E: ToolExecutor, R: RecallInjector> Session<E, R> {
    pub fn with_system(mut self, prompt: impl Into<String>) -> Self {
        self.history = vec![Message::system(prompt)];
        self
    }

    pub fn with_executor<E2: ToolExecutor>(self, executor: E2) -> Session<E2, R> {
        Session {
            client: self.client,
            history: self.history,
            window: self.window,
            executor,
            recall: self.recall,
        }
    }

    pub fn with_recall<R2: RecallInjector>(self, recall: R2) -> Session<E, R2> {
        Session {
            client: self.client,
            history: self.history,
            window: self.window,
            executor: self.executor,
            recall,
        }
    }

    pub fn history(&self) -> &[Message] {
        &self.history
    }

    #[cfg(test)]
    pub fn seed(&mut self, extra: impl IntoIterator<Item = Message>) {
        self.history.extend(extra);
    }

    pub async fn turn(
        &mut self,
        user_text: &str,
        cancel: &Cancellation,
        mut on_token: impl FnMut(&str),
    ) -> Result<String, CompanionError> {
        self.history.push(Message::user(user_text));
        let keep = self.history.len();
        let mut rounds = 0;
        loop {
            if cancel.is_cancelled() {
                self.history.truncate(keep);
                return Err(CompanionError::Cancelled);
            }
            let packed = match pack_messages(&self.history, self.window, &self.recall) {
                Ok(packed) => packed,
                Err(error) => {
                    self.history.truncate(keep);
                    return Err(error);
                }
            };
            let specs = self.executor.specs();
            let completion = match self
                .client
                .complete(&packed.messages, &specs, cancel, &mut on_token)
                .await
            {
                Ok(completion) => completion,
                Err(error) => {
                    self.history.truncate(keep);
                    return Err(error);
                }
            };
            match completion.finish {
                Finish::Stop => {
                    self.history
                        .push(Message::assistant(completion.content.clone(), Vec::new()));
                    return Ok(completion.content);
                }
                Finish::ToolCalls => {
                    rounds += 1;
                    if rounds > MAX_TOOL_ROUNDS {
                        self.history.truncate(keep);
                        return Err(CompanionError::ToolLoop);
                    }
                    let calls = completion.tool_calls;
                    self.history
                        .push(Message::assistant(completion.content, calls.clone()));
                    for call in calls {
                        if cancel.is_cancelled() {
                            self.history.truncate(keep);
                            return Err(CompanionError::Cancelled);
                        }
                        let result = match parse_tool_object(&call.arguments) {
                            Ok(args) if specs.iter().any(|spec| spec.name == call.name) => {
                                self.executor.execute(&call.name, &args)
                            }
                            Ok(_) => text::get("companion.unknown_tool").to_owned(),
                            Err(()) => text::get("companion.malformed_tool_args").to_owned(),
                        };
                        self.history.push(Message::tool(call.id, result));
                    }
                }
            }
        }
    }
}
