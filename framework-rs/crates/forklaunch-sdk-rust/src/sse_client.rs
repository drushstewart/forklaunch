/// A bounded, pull-driven event stream. Dropping or closing it releases the response.
#[derive(Debug)]
pub struct SdkEventStream<T> {
    response: Option<reqwest::Response>,
    decoder: sse::SseDecoder,
    pending: std::collections::VecDeque<sse::SseEvent>,
    marker: std::marker::PhantomData<T>,
}
impl<T: serde::de::DeserializeOwned> SdkEventStream<T> {
    pub fn from_response(
        response: reqwest::Response,
        max_event_bytes: usize,
    ) -> Result<Self, SdkError> {
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("text/event-stream")
        {
            return Err(SdkError::Stream("Expected text/event-stream response"));
        }
        Ok(Self {
            response: Some(response),
            decoder: sse::SseDecoder::new(max_event_bytes)?,
            pending: Default::default(),
            marker: Default::default(),
        })
    }

    pub fn close(&mut self) {
        self.response = None;
        self.pending.clear();
        self.decoder.finish();
    }

    pub async fn next(&mut self) -> Result<Option<T>, SdkError> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                let data = serde_json::from_str(&event.data)
                    .unwrap_or(serde_json::Value::String(event.data));
                let mut value = serde_json::json!({"data": data});
                if let Some(id) = event.id {
                    value["id"] = serde_json::Value::String(id);
                }
                match serde_json::from_value(value) {
                    Ok(value) => return Ok(Some(value)),
                    Err(error) => {
                        self.close();
                        return Err(error.into());
                    }
                }
            }
            if self.response.is_some()
                && let Some(error) = self.decoder.failure()
            {
                self.close();
                return Err(error.into());
            }
            let Some(response) = self.response.as_mut() else {
                return Ok(None);
            };
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => {
                    self.close();
                    return Ok(None);
                }
                Err(error) => {
                    self.close();
                    return Err(error.into());
                }
            };
            match self.decoder.push(&chunk) {
                Ok(events) => self.pending.extend(events),
                Err(error) => {
                    self.close();
                    return Err(error.into());
                }
            }
        }
    }
}
