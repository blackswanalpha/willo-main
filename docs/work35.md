# work35 — M35: App suite, productivity (mail + calendar via JMAP/IMAP/CalDAV)

> Derived from `docs/idea.md` §21 (Application Suite), §13 (Multi-user — accounts).

## Goal

Ship a unified mail + calendar + contacts app (`willomail` + `willocal` + `willocontacts`) with a background sync daemon, local SQLite cache, and event-driven UI updates. **JMAP** is the preferred protocol (JSON-over-HTTPS, batch ops, push); **IMAP+SMTP** fallback for legacy hosts; **CalDAV/CardDAV** for calendars/contacts. OAuth/passwords come from §28 online-accounts.

## Depends on

- **M14** — netstack + rustls.
- **M28** — online-accounts (OAuth tokens, app passwords).
- **M30** — UTF-8 + locale.
- **M19** — packaging.
- **M37** — multi-user (per-user data dirs).

## Acceptance criteria

- [ ] `willomail` lists, reads, sends mail against a JMAP server.
- [ ] IMAP fallback works for an account with no JMAP support; SMTP submits with STARTTLS.
- [ ] `willocal` syncs CalDAV calendars; recurring events expand correctly (RRULE).
- [ ] `willocontacts` syncs CardDAV; merges duplicate contacts.
- [ ] All three apps share a SQLite cache and an in-process event bus.
- [ ] Push notification on JMAP delivers new mail toast within 5 s.
- [ ] Offline write (compose + queue) is delivered on reconnect.
- [ ] `userspace/willomail/tests/jmap_send_recv.rs` round-trips a message via fakeserver.

## Task breakdown

### T1. Account integration — `userspace/willomail/accounts.rs`
- D-Bus client to §28 `org.willo.OnlineAccounts`.
- Per-account config: protocol (JMAP/IMAP), endpoints, credentials handle.

### T2. JMAP client — `userspace/willomail/jmap/`
- `Mailbox/get`, `Email/query`, `Email/get`, `Email/changes`, `EmailSubmission/set`.
- WebSocket push via `EventSource`-style.

### T3. IMAP+SMTP fallback — `userspace/willomail/imap/`, `smtp/`
- Stateful IMAP per mailbox; FETCH/SEARCH/STORE.
- SMTP submit with AUTH PLAIN over STARTTLS.

### T4. CalDAV/CardDAV — `userspace/willocal/dav/`, `userspace/willocontacts/dav/`
- WebDAV PROPFIND/REPORT; ICS / VCF parsing.
- iCalendar RRULE expander (rrule-rs class) with fuzz vectors.

### T5. Local cache — `userspace/willomail/cache.rs`
- SQLite schema: mailboxes, emails (header-only first), bodies-on-demand, calendars, events, contacts.
- WAL mode; per-account isolation.

### T6. Sync daemon — `userspace/willomail-sync/`
- One sync loop per account; backoff on errors.
- Triggers: timer (poll), JMAP push, manual refresh.
- D-Bus signals for UI refresh.

### T7. UI shells — `userspace/willomail/ui.rs` etc.
- Compositor clients via `iced-willo`.
- Three apps share a "messaging core" crate for SQLite + bus.

### T8. Notifications — integrate §36 settings + compositor toast surface
- Per-app notification policy (sound, banner duration).
- Quiet hours via §28 schedule.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/willomail/` | **new** |
| `userspace/willocal/` | **new** |
| `userspace/willocontacts/` | **new** |
| `userspace/willomail-sync/` | **new** (shared daemon) |
| `userspace/willoc-notify/` | **new** notification daemon |

## Tests to add

- `userspace/willomail/tests/jmap_send_recv.rs` — round-trip via stub server.
- `userspace/willomail/tests/imap_fallback.rs` — IMAP path covers FETCH/STORE.
- `userspace/willocal/tests/rrule_expand.rs` — IETF iCalendar test vectors.
- `userspace/willocontacts/tests/dedup_merge.rs` — duplicate VCF entries merged.
- `userspace/willomail-sync/tests/offline_compose.rs` — compose offline, deliver on reconnect.

## Risks & open questions

- **JMAP server availability** — Fastmail, Stalwart, others; few in 2026; IMAP fallback essential.
- **RRULE complexity** — fuzz against IETF/Apple test vectors before shipping.
- **OAuth refresh on §23 sleep** — wake-up handler must revalidate before push.
- **Body cache size** — bodies are bulky; cap cache size + prune by LRU.
- **Spam filtering** — out of scope v1; document a future hook.
- **HTML email** — render via mini-WebView (subset of §24 browser); sandbox carefully.
