A simple [Delivery SMTP server](https://datatracker.ietf.org/doc/html/rfc5321#section-2.3.10) to assist in development and testing of software that requires email accounts (i.e. identity management). There's no accounts to manage. All emails will be saved to file. You can therefore use a different email "account" for every automated test run for example.

## Requirements

- A server that is reachable via the public internet
- A domain name and access to its DNS configuration

## Setup

Let's assume that you want to receive emails sent to the `@dev1.mail.example.com` and `@dev2.mail.example.com` domains.

### DNS

You will need 3 DNS records:

```
mydevserver.example.com   A    <ip of your server>
dev1.mail.example.com     MX   10   mydevserver.example.com
dev2.mail.example.com     MX   10   mydevserver.example.com
```

### System Configuration

SMTP delivery runs on port 25. On Linux, only root can bind to this port.

### App Configuration

#### 1. Create a `.env` file that looks like this

```
API_KEY=averylongpasswordtoprotectadminapi
EMAIL_DOMAIN=dev1.mail.example.com,dev2.mail.example.com
EMAIL_ACCOUNT_PREFIX=goodmailonly-
ADMIN_APP_PORT=2255
```

Emails sent to addresses that don't start with the prefix will be ignored. This provides some crude protection against spam. In the above example, an email sent to `goodmailonly-test-MCPNBoXE@dev1.mail.example.com` will be saved to file. An email sent to `test-MCPNBoXE@dev1.mail.example.com` will be rejected.

If there is no email domain or email account prefix set, all e-mails will be accepted.

#### 2. Run `docker` to start the container

```
docker run -p 25:25 -v $(pwd)/.env:/.env -v $(pwd)/mail:/mail darktohka/mail-receiver-rs
```

# API Access

You can get a list of all received emails by week using the admin api: `/api/mail/<year>/<week-number>?api_key=<configured setting>`. The weekly view provides API links to examine individual messages.

# Testing your setup / Sending Email

Testing this tool by using the SMTP protocol to directly call port 25 can be tricky since many ISPs and service providers (e.g. AWS) block outgoing traffic on port 25. Therefore, you'll need a proper SMTP gateway to send messages. You cand easily test your setup by manually sending emails from say gmail.
