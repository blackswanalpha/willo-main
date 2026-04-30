# flow35 — M35: architecture & runtime flows (Mail + calendar + contacts)

> Derived from `docs/idea.md` §21, §13 and the work35 plan.

## Component map

```
        +-------------+ +-------------+ +-----------------+
        |  willomail  | |  willocal   | |  willocontacts  |
        +------+------+ +------+------+ +--------+--------+
               |               |                 |
               +-------+-------+--------+--------+
                       | shared messaging core
                       v
        +-----------------------------+
        |  willomail-sync (daemon)    |
        |   per-account loop          |
        +--+------+------+------+-----+
           |      |      |      |
           v      v      v      v
        JMAP   IMAP   CalDAV  CardDAV
           \      \      /      /
            \      \    /      /
             v      v  v      v
                §14 netstack + rustls
                §28 online-accounts (creds)
        +-----------------------------+
        |  SQLite cache (per user)    |
        +-----------------------------+
```

## JMAP fetch flow

```
sync loop:
  call JMAP method batch:
    [
      Mailbox/get,
      Email/query (filter sortedBy=receivedAt desc, limit=100),
      Email/get (ids from prior step, properties=headers)
    ]
  parse JSON response; one HTTP round-trip
  upsert into SQLite cache (mailboxes, emails)
  emit D-Bus signal "messages_changed" to UI
```

## JMAP push (incoming mail)

```
WebSocket open to /eventsource (or pushSubscribe)
  remote: PushNotification { accountId, type=Mailbox|Email, changedProperties=[...] }
  daemon: trigger refresh for account; fetch deltas
  on new mail:
     §36 notification: "1 new from Alice — Subject"
```

## IMAP fallback

```
sync loop (IMAP):
  CONNECT; STARTTLS; LOGIN
  SELECT INBOX
  UID FETCH 1:* (UID FLAGS RFC822.HEADER)
     -> upsert headers
  IDLE -> server pushes "EXISTS" / "EXPUNGE"
     -> wake; FETCH new
  on send: SMTP submission separately
```

## SMTP submit

```
willomail compose -> EmailSubmission
  via JMAP: EmailSubmission/set
  via SMTP fallback:
     CONNECT submission server (587)
     EHLO; STARTTLS; AUTH PLAIN
     MAIL FROM; RCPT TO; DATA . QUIT
  if offline: enqueue in SQLite outbox; retry on reconnect
```

## CalDAV sync

```
PROPFIND /calendars/{user}/{cal}/ -> resource list
REPORT calendar-multiget -> ICS bodies
parse ICS:
  for VEVENT:
    if RRULE present:
       rrule_expand(vevent.dtstart, rrule, until=now+1y)
       upsert all instances + master into events table
emit "calendar_changed" D-Bus
```

## Offline compose

```
willomail compose -> "send"
  -> save in outbox table with state=Pending
  -> if online: trigger sync; deliver via JMAP/SMTP; mark Sent
  -> if offline: state stays Pending
on §23 resume / network back:
  sync daemon scans outbox; delivers each
```

## Notification flow

```
new mail arrives via push
  -> sync daemon checks settings:
       quiet_hours? -> queue silently
       else -> willoc-notify toast (compositor surface)
       sound effect? -> §18 audio
```

## Failure paths

- **JMAP 401** → mark account "needs reauth"; UI prompt; backoff sync.
- **IMAP TLS fail** → no plain fallback; sync stops; UI warns "TLS required".
- **RRULE expand explosion** (yearly daily for 100 years) → cap expansion at 5y forward + error.
- **SQLite WAL too big** → checkpoint hourly; cap at 64 MiB.
- **OAuth refresh transient fail** → backoff (1m, 5m, 30m); after 3 fails, mark "needs reauth".

## Data structures

```rust
pub struct Account {
    pub id: AccountId,
    pub name: String,
    pub kind: AccountKind,                // Jmap | Imap | CalDav | CardDav
    pub creds: AccountsHandle,            // §28
    pub endpoints: Endpoints,
}

pub struct EmailRow {
    pub id: String,                       // server-side id (JMAP) or UID (IMAP)
    pub mailbox_id: String,
    pub thread_id: Option<String>,
    pub from: Vec<EmailAddress>,
    pub to: Vec<EmailAddress>,
    pub subject: String,
    pub received_at: u64,
    pub flags: u32,                       // Read | Flagged | Deleted | Draft
    pub size: u64,
    pub body_cached: bool,
}

pub struct CalEvent {
    pub uid: String,
    pub calendar_id: String,
    pub dtstart: u64,
    pub dtend: u64,
    pub rrule: Option<RRule>,
    pub master_uid: Option<String>,       // for instances
    pub summary: String,
    pub location: Option<String>,
}

pub struct OutboxEntry {
    pub id: u64,
    pub account_id: AccountId,
    pub mime_bytes: Vec<u8>,
    pub state: OutboxState,               // Pending | Sending | Sent | Failed
    pub attempts: u8,
}
```
