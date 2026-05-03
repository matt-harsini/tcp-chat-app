# tokio-chat

Fun project over the weekend to learn about tokio semantics against traditional mutexes in distributed computing.

## What I learned

Built incrementally across small milestones, each chosen to teach a specific concept by feel.

**M1 — Multi-client broadcast.** Task-per-connection, with the list of write halves behind `Arc<Mutex<Vec<...>>>`. Worked, but introduced the classic Tokio anti-pattern of holding a lock across `.await` — one slow client could stall delivery to everyone else.

**M2 — Channels over locks.** Swapped the shared `Vec` for `tokio::sync::broadcast`. Each task subscribes and races its socket read against `receiver.recv()` in a `select!`. No locks, no shared state. Learned the difference between `Lagged` (soft, continue) and `Closed` (hard, return) on broadcast receivers.

**M3 — Line framing, usernames, and clean disconnects.** Added `BufReader` + `read_line` for `\n`-delimited framing on both ends. Implemented a username handshake before the steady-state loop, plus a `/quit` command. The big conceptual lesson: TCP is a byte stream, not a message stream — framing is the protocol layer that defines what counts as a message. Also learned to detect peer disconnects via `Ok(0)` on read and exit cleanly instead of `?`-propagating I/O errors out of `main`.

**M4 — Actor pattern.** Broadcast carries values but not identities, so it can't support `/dm <user>`. Replaced the broadcast bus with a single router task owning a `HashMap<String, mpsc::Sender<String>>`. Connections talk to the router via a `RouterCommand` enum (`Join`, `Leave`, `Broadcast`, `Direct`); the router does fan-out by iterating the map. Used `try_send` (not `.await`) inside the router so one slow recipient can't stall the dispatcher — the M2 lesson reapplied to the new shape.
