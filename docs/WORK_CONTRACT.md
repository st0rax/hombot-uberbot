# Arbeitsleitfaden (Bazaar, HomBot)

Teilnahme ist freiwillig. Dummy-Bazaar erklärt das **Wie** generisch. Dieses
Repo hat **strengeren Schutz**. Bei Konflikt gilt die strengere HomBot-Regel
(`AGENTS.md`, `STATUS_LIVE.md`, `SECURITY.md`).

## Claim

- Nur in `docs/TASKBOARD.json`: `status=claimed`, `owner`, `branch`,
  `claimed_at`.
- Eine JSON-`id`, ein Entwickler.
- G-001 ist kein Claim.
- Kein Claim, der SSH, Deploy, `Name.dat`, UART-Attach oder Motoren braucht,
  solange Device-Eis gilt (`STATUS_LIVE.md` maintenance window).

## Eine Sache

Zweig von `main`: `feature/<id>-<kurz>` oder `docs/<id>-<kurz>`. Klein mergen.
PR #3 und PR #4 nicht nebenbei mergen.

## Verifikation

`cargo test --lib` ist ein Code-Gate, **kein** Robot-Gate.

- Tree-only: Tests bzw. der in der Zelle genannte Befehl, plus PR.
- Live: nur mit neuer `STATUS_LIVE.md`-Zeile, die eine Messung auf dem Gerät
  nennt. Decoded ≠ live. Ice: keine Live-Zelle auf `done`.

## Abnahme

- [ ] `AGENTS.md`, `STATUS_LIVE.md`, `SECURITY.md` gelesen
- [ ] Claim vollständig
- [ ] Zelle-`verification` erfüllt
- [ ] keine Secrets, Hosts, Dumps
- [ ] nichts als live markiert, das nicht gemessen wurde
- [ ] Motorzellen weiter `blocked`, solange Interlocks/UART nicht `done` sind
- [ ] Working tree / PR sauber

Mängel zurück an den Claim. Grober Ausreißer: Zelle auf `free`, Stand
rückwärts per neuem Commit.
