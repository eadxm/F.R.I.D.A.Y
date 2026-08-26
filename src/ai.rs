use reqwest::Client;

pub struct FridayBrain {
    pub client: Client,
    pub api_key: String,
}

impl FridayBrain {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    pub async fn process_prompt(&self, _prompt: &str) -> String {
        // TODO: We will write the Gemini 3.7 Flash API HTTP request here next
        String::from("I am online and ready.")
    }
}