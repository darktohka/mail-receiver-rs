use mail_parser::{MessageParser, MimeHeaders};

const EXAMPLE_EMAIL: &str = "ARC-Seal: i=1; a=rsa-sha256; t=1778744669; cv=none; \r\n\
Received: from mail.zoho.com by mx.zohomail.com\r\n\
\twith SMTP id 1778744668240152.48471785341417; Thu, 14 May 2026 00:44:28 -0700 (PDT)\r\n\
Date: Thu, 14 May 2026 10:44:28 +0300\r\n\
From: Daniel D <daniel@tohka.us>\r\n\
To: \"test\" <test@tsv.tohka.us>\r\n\
Message-Id: <19e2571b03f.16b673da262918.5024988905688513063@tohka.us>\r\n\
Subject: Here is a test with an attachment\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; \r\n\
\tboundary=\"----=_Part_666597_1393469714.1778744668223\"\r\n\
Importance: Medium\r\n\
User-Agent: Zoho Mail\r\n\
X-Mailer: Zoho Mail\r\n\
X-Zoho-Virus-Status: 1\r\n\
X-Zoho-AV-Stamp: zmail-av-0.2.2.1.5.2/278.732.11\r\n\
\r\n\
------=_Part_666597_1393469714.1778744668223\r\n\
Content-Type: multipart/alternative; \r\n\
\tboundary=\"----=_Part_666598_143185156.1778744668223\"\r\n\
\r\n\
------=_Part_666598_143185156.1778744668223\r\n\
Content-Type: text/plain; charset=\"UTF-8\"\r\n\
Content-Transfer-Encoding: 7bit\r\n\
\r\n\
Hello.\r\n\
------=_Part_666598_143185156.1778744668223\r\n\
Content-Type: text/html; charset=\"UTF-8\"\r\n\
Content-Transfer-Encoding: 7bit\r\n\
\r\n\
<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\"><html><head><meta content=\"text/html;charset=UTF-8\" http-equiv=\"Content-Type\"></head><body ><div style=\"font-family: Verdana, Arial, Helvetica, sans-serif; font-size: 10pt;\">Hello.</div><br></body></html>\r\n\
------=_Part_666598_143185156.1778744668223--\r\n\
\r\n\
------=_Part_666597_1393469714.1778744668223\r\n\
Content-Type: application/octet-stream; name=README.md\r\n\
Content-Transfer-Encoding: 7bit\r\n\
X-ZM_AttachId: 139980374682230460\r\n\
Content-Disposition: attachment; filename=README.md\r\n\
\r\n\
A simple [Delivery SMTP server](https://datatracker.ietf.org/doc/html/rfc5321#section-2.3.10) to assist in development and testing of software that requires email accounts (i.e. identity management). There's no accounts to manage. All emails will be saved to file. You can therefore use a different email \"account\" for every automated test run for example.\r\n\
\r\n\
## Requirements\r\n\
\r\n\
- A server that is reachable via the public internet\r\n\
- A domain name and access to its DNS configuration\r\n\
------=_Part_666597_1393469714.1778744668223--\r\n";

const README_CONTENT: &str = r#"A simple [Delivery SMTP server](https://datatracker.ietf.org/doc/html/rfc5321#section-2.3.10) to assist in development and testing of software that requires email accounts (i.e. identity management). There's no accounts to manage. All emails will be saved to file. You can therefore use a different email "account" for every automated test run for example.

## Requirements

- A server that is reachable via the public internet
- A domain name and access to its DNS configuration
"#;

#[test]
fn test_parse_example_email_attachments() {
    let msg = MessageParser::default()
        .parse(EXAMPLE_EMAIL.as_bytes())
        .expect("should parse the example email");

    // ── Body text ──
    let text_body = msg.body_text(0).map(|c| c.to_string());
    assert_eq!(text_body.as_deref(), Some("Hello."), "text body should be 'Hello.'");

    // ── Body HTML ──
    let html_body = msg.body_html(0).map(|c| c.to_string());
    assert!(html_body.is_some(), "html body should be present");
    assert!(
        html_body.as_deref().unwrap().contains("Hello."),
        "html body should contain 'Hello.'"
    );

    // ── Attachments ──
    let attachments: Vec<_> = msg.attachments().collect();
    assert_eq!(
        attachments.len(),
        1,
        "should have exactly 1 attachment, got {}",
        attachments.len()
    );

    let att = &attachments[0];

    // Filename
    let filename = att.attachment_name().map(|s| s.to_string());
    assert_eq!(
        filename.as_deref(),
        Some("README.md"),
        "attachment filename should be README.md"
    );

    // Content-Type
    let content_type = att.content_type().map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")));
    assert_eq!(
        content_type.as_deref(),
        Some("application/octet-stream"),
        "content type should be application/octet-stream"
    );

    // Content-Disposition -> inline flag
    let has_content_disposition = att.content_disposition().is_some();
    assert!(has_content_disposition, "attachment should have Content-Disposition header");
    let is_inline = att
        .content_disposition()
        .is_some_and(|d| d.ctype() != "attachment");
    assert!(!is_inline, "attachment should NOT be inline (disposition = attachment)");

    // No Content-ID
    assert!(att.content_id().is_none(), "attachment should have no Content-ID");

    // Content (normalize CRLF -> LF since raw email uses SMTP \r\n)
    let attachment_text = std::str::from_utf8(att.contents()).expect("attachment content should be valid UTF-8");
    let attachment_normalized = attachment_text.replace("\r\n", "\n");
    assert_eq!(
        attachment_normalized.trim_end(),
        README_CONTENT.trim_end(),
        "attachment content should match the README.md file content"
    );

    // ── Also test via the ParsedMail conversion ──
    let parsed = mail_receiver_rs::types::ParsedMail::from(&msg);
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(
        parsed.attachments[0].filename.as_deref(),
        Some("README.md")
    );
    assert_eq!(
        parsed.attachments[0].content_type.as_deref(),
        Some("application/octet-stream")
    );
    assert!(!parsed.attachments[0].inline);
    assert!(parsed.attachments[0].content_id.is_none());
    assert_eq!(parsed.attachments[0].size, att.contents().len());
    assert_eq!(parsed.attachments[0].index, 0);
}
