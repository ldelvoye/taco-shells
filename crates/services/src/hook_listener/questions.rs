use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

use taco_protocol::routes::STOP;
use taco_protocol::{ContextAnswer, HookMessage, StopAnswer};

use super::http::write_response;
use super::messages::parse_hook_message;
use super::{ListenerEvent, Serving};

const NO_ANSWER: &str = "{}";

#[derive(Clone, Debug, PartialEq)]
pub enum QuestionKind {
    Context { source: String },
    Stop,
}

pub enum AnswerPart {
    Placed(bool),
    Context(Option<String>),
    Nudge(Option<String>, Option<OnDelivered>),
}

pub type OnDelivered = Box<dyn FnOnce() + Send>;

#[derive(Clone, Debug)]
pub struct Question {
    pub kind: QuestionKind,
    pub message: HookMessage,
    pub reply: Sender<AnswerPart>,
}

impl PartialEq for Question {
    fn eq(&self, other: &Question) -> bool {
        self.kind == other.kind && self.message == other.message
    }
}

struct Answer {
    body: String,
    on_delivered: Option<OnDelivered>,
}

struct Pending {
    placed: Option<bool>,
    content: Option<Answer>,
}

impl Pending {
    fn is_complete(&self) -> bool {
        if self.placed == Some(false) {
            return true;
        }
        self.placed == Some(true) && self.content.is_some()
    }

    fn take(&mut self, kind: &QuestionKind, part: AnswerPart) {
        if let AnswerPart::Placed(is_placed) = part {
            self.placed = Some(is_placed);
            return;
        }
        let held = complete_answer(kind, part);
        if held.is_some() {
            self.content = held;
        }
    }

    fn into_answer(self, kind: &QuestionKind) -> Answer {
        let nothing = Answer {
            body: String::from(NO_ANSWER),
            on_delivered: None,
        };
        if self.placed != Some(true) {
            return nothing;
        }
        if let Some(answer) = self.content {
            return answer;
        }
        if matches!(kind, QuestionKind::Context { .. }) {
            return Answer {
                body: context_body(None),
                on_delivered: None,
            };
        }
        nothing
    }
}

pub(super) fn ask_question(
    mut stream: TcpStream,
    route: &str,
    sent_token: Option<&str>,
    body: &[u8],
    answer_by: Instant,
    serving: &Serving,
) {
    let token = serving.token.as_str();
    let on_event = &serving.on_event;
    if sent_token != Some(token) {
        write_response(&mut stream, 200, Some(NO_ANSWER));
        return;
    }
    let Some(parsed) = parse_hook_message(body, sent_token, token) else {
        write_response(&mut stream, 200, Some(NO_ANSWER));
        return;
    };

    let kind = question_kind(route, parsed.source);
    let (reply, replies) = mpsc::channel();
    let question = Question {
        kind: kind.clone(),
        message: parsed.message,
        reply,
    };
    on_event(ListenerEvent::Question(question));

    // Answered off the serial loop, so a question waiting on the workspace holds up no other post.
    let spare = stream.try_clone();
    let builder = thread::Builder::new().name("hook-question".into());
    let spawned = builder.spawn(move || answer_question(stream, &kind, &replies, answer_by));
    if spawned.is_err()
        && let Ok(mut spare) = spare
    {
        write_response(&mut spare, 200, Some(NO_ANSWER));
    }
}

fn question_kind(route: &str, source: String) -> QuestionKind {
    if route == STOP {
        return QuestionKind::Stop;
    }
    QuestionKind::Context { source }
}

fn answer_question(
    mut stream: TcpStream,
    kind: &QuestionKind,
    replies: &Receiver<AnswerPart>,
    answer_by: Instant,
) {
    let mut pending = Pending {
        placed: None,
        content: None,
    };
    loop {
        if pending.is_complete() {
            break;
        }
        let left = answer_by.saturating_duration_since(Instant::now());
        let Ok(part) = replies.recv_timeout(left) else {
            break;
        };
        pending.take(kind, part);
    }

    let answer = pending.into_answer(kind);
    let written = write_response(&mut stream, 200, Some(&answer.body));
    if written && let Some(on_delivered) = answer.on_delivered {
        on_delivered();
    }
}

fn context_body(context: Option<String>) -> String {
    let answer = ContextAnswer {
        yours: true,
        context,
    };
    match serde_json::to_string(&answer) {
        Ok(text) => text,
        Err(_) => String::from(NO_ANSWER),
    }
}

fn stop_body(reason: Option<String>) -> String {
    let answer = StopAnswer { reason };
    match serde_json::to_string(&answer) {
        Ok(text) => text,
        Err(_) => String::from(NO_ANSWER),
    }
}

fn complete_answer(kind: &QuestionKind, part: AnswerPart) -> Option<Answer> {
    match (kind, part) {
        (QuestionKind::Context { .. }, AnswerPart::Context(context)) => Some(Answer {
            body: context_body(context),
            on_delivered: None,
        }),
        (QuestionKind::Stop, AnswerPart::Nudge(Some(reason), on_delivered)) => Some(Answer {
            body: stop_body(Some(reason)),
            on_delivered,
        }),
        (QuestionKind::Stop, AnswerPart::Nudge(None, _)) => Some(Answer {
            body: stop_body(None),
            on_delivered: None,
        }),
        _ => None,
    }
}
