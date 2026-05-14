use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use mail_parser::MimeHeaders;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientInfo {
    pub domain: String,
    pub name: String,
    pub email: String,
    pub message_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSummary {
    pub recipient_folder_path: String,
    pub message_id: String,
    pub processed_at: String,
    pub from: Option<String>,
    pub subject: Option<String>,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyIndex {
    pub name: String,
    pub messages: Vec<TransactionSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub index: u32,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: usize,
    pub content_id: Option<String>,
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedMail {
    pub attachments: Vec<AttachmentInfo>,
    pub headers: Map<String, Value>,
    pub header_lines: Vec<HeaderLine>,
    pub html: Option<String>,
    pub text: Option<String>,
    pub text_as_html: Option<String>,
    pub subject: Option<String>,
    pub date: Option<String>,
    pub to: Option<AddressObject>,
    pub from: Option<AddressObject>,
    pub cc: Option<AddressObject>,
    pub bcc: Option<AddressObject>,
    pub reply_to: Option<AddressObject>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeaderLine {
    pub key: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressObject {
    pub value: Vec<Address>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Address {
    pub address: String,
    pub name: String,
}

impl<'x> From<&mail_parser::Message<'x>> for ParsedMail {
    fn from(msg: &mail_parser::Message<'x>) -> Self {
        let raw_msg = &msg.raw_message;
        let header_lines: Vec<HeaderLine> = msg
            .headers()
            .iter()
            .map(|h| {
                let raw_line = std::str::from_utf8(
                    &raw_msg[h.offset_field() as usize..h.offset_end() as usize],
                )
                .unwrap_or("");
                HeaderLine {
                    key: h.name().to_lowercase(),
                    line: raw_line.trim_end_matches(&['\r', '\n'][..]).to_string(),
                }
            })
            .collect();

        let headers: Map<String, Value> = Map::new();

        let to = msg.to().map(address_to_object);
        let from = msg.from().map(address_to_object);
        let cc = msg.cc().map(address_to_object);
        let bcc = msg.bcc().map(address_to_object);
        let reply_to = msg.reply_to().map(address_to_object);

        let date = msg.date().map(|d| {
            let ts = d.to_timestamp();
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| format!("{}", dt.format("%Y-%m-%dT%H:%M:%S.000Z")))
                .unwrap_or_else(|| d.to_rfc3339())
        });

        let html_body = msg.body_html(0).map(|c| c.to_string());
        let text_body = msg.body_text(0).map(|c| c.to_string());
        let text_as_html = text_body.as_ref().map(|t| text_to_text_as_html(t));

        let message_id = msg.message_id().map(|s| format!("<{s}>"));

        let in_reply_to = msg.header_raw("In-Reply-To").map(|s| s.to_string());
        let references = msg.header_raw("References").map(|s| s.to_string());

        let attachments: Vec<AttachmentInfo> = msg
            .attachments()
            .enumerate()
            .map(|(i, part)| {
                let ct = part.content_type().map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")));
                let is_inline = part
                    .content_disposition()
                    .is_some_and(|d| d.ctype() != "attachment");
                AttachmentInfo {
                    index: i as u32,
                    filename: part.attachment_name().map(|s| s.to_string()),
                    content_type: ct,
                    size: part.contents().len(),
                    content_id: part.content_id().map(|s| s.to_string()),
                    inline: is_inline,
                }
            })
            .collect();

        ParsedMail {
            attachments,
            headers,
            header_lines,
            html: html_body,
            text: text_body,
            text_as_html,
            subject: msg.subject().map(|s| s.to_string()),
            date,
            to,
            from,
            cc,
            bcc,
            reply_to,
            message_id,
            in_reply_to,
            references,
        }
    }
}

fn address_to_object(addr: &mail_parser::Address) -> AddressObject {
    let mut value = Vec::new();
    let mut text_parts = Vec::new();

    for a in addr.iter() {
        let name = a.name().unwrap_or("").to_string();
        let address = a.address().unwrap_or("").to_string();

        value.push(Address {
            address: address.clone(),
            name: if name.is_empty() {
                String::new()
            } else {
                name.clone()
            },
        });

        if name.is_empty() {
            text_parts.push(address.clone());
        } else {
            text_parts.push(format!("\"{name}\" <{address}>"));
        }
    }

    AddressObject {
        value,
        text: text_parts.join(", "),
    }
}

fn text_to_text_as_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 32);
    let mut in_para = false;

    for line in text.lines() {
        if line.trim().is_empty() {
            if in_para {
                result.push_str("</p>");
                in_para = false;
            }
        } else {
            if !in_para {
                result.push_str("<p>");
                in_para = true;
            } else {
                result.push_str("<br/>");
            }
            for ch in line.chars() {
                match ch {
                    '<' => result.push_str("&lt;"),
                    '>' => result.push_str("&gt;"),
                    '&' => result.push_str("&amp;"),
                    '"' => result.push_str("&quot;"),
                    _ => result.push(ch),
                }
            }
        }
    }
    if in_para {
        result.push_str("</p>");
    }
    result
}
