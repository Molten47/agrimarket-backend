use resend_rs::{Resend, types::CreateEmailBaseOptions};
use crate::templates::EmailTemplate;

#[derive(Clone)]
pub struct MailerService {
    client:    Resend,
    from:      String,
    from_name: String,
}

impl MailerService {
    pub fn new(api_key: &str, from: &str, from_name: &str) -> Self {
        Self {
            client:    Resend::new(api_key),
            from:      from.to_string(),
            from_name: from_name.to_string(),
        }
    }

    pub async fn send(&self, to: &str, template: EmailTemplate) -> anyhow::Result<()> {
        let from = format!("{} <{}>", self.from_name, self.from);

        let email = CreateEmailBaseOptions::new(from, vec![to], template.subject())
            .with_html(&template.html_body());

        self.client.emails.send(email).await?;
        Ok(())
    }
}
