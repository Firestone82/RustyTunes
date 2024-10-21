use async_trait::async_trait;
use songbird::{Event, EventContext, EventHandler};

pub struct ErrorHandler;

#[async_trait]
impl EventHandler for ErrorHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        println!("Error detected. Error handler called to action.");
        None
    }
}