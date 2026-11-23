# Versjonslogg

## 2.0.1 — 2026-11-23

- Vinduet åpner midt på skjermen igjen når du henter det frem. Etter at minimer-knappen var brukt én gang, havnet vinduet oppe i venstre hjørne når det ble åpnet fra appoversikten, og fra systemfeltet kom det ikke frem i det hele tatt.
- Innstillinger og tegnevinduet kommer ikke lenger tilbake minimert etter et skjermbilde bare fordi de har vært minimert én gang før.
- Når Agentic Screenshot-utvidelsen er slått på, men GNOME ikke har lastet den ennå, sier appen fra én gang om at GNOME tok bildet med blits og lyd, og at stille opptak virker når du har logget ut og inn. Innstillinger sier det samme under Opptaksmåte, og Blits og Lukkerlyd er grå til utvidelsen kjører.
- Hurtigtasten tar ikke lenger det første bildet med blits og lyd når den må starte appen først.
- Ctrl+A, Delete og de andre snarveiene i hovedvinduet gjør ingenting mens Innstillinger eller tegnevinduet er åpent.
- Søppelbøtta på et bilde er skjult også for skjermlesere mens du velger flere bilder.
- Et frihåndsklipp som bare er en strek, avvises som for lite i stedet for å lagre et tomt bilde.
- Tegnevinduet får plass på små skjermer og på halve skjermen. Verktøylinja ruller sidelengs når det ikke er plass til hele.
- Lagring i tegnevinduet beholder de utvidede rettighetene (ACL) på filen.

## 2.0.0 — 2026-11-23

- Agentic Klipp heter nå Agentic Screenshot. Binæren heter `agentic-screenshot`, app-ID-en `io.github.AgenticScreenshot` og Shell-utvidelsen `agentic-screenshot@bejulian1.github.io`.
- Første gang den nye versjonen starter, flyttes innstillingene, hylla og cachen automatisk fra `~/.config/klipp`, `~/.local/state/klipp` og `~/.cache/klipp` til mapper som heter `agentic-screenshot`. Autostart og egne GNOME-hurtigtaster som startet `klipp`, peker videre på den nye binæren, og den nye utvidelsen slås på hvis den gamle var på.
- Bildene flyttes aldri. Brukte du standardmappen `Bilder/Klipp`, havner nye bilder der også etter oppgraderingen. Nye installasjoner lagrer i `Bilder/Agentic Screenshot`, med filnavn som begynner på `screenshot-`.
- install.sh fjerner den gamle binæren, .desktop-filen, ikonene, autostart-oppføringen og utvidelsen før den nye installeres.
- Logg ut og inn én gang etter oppgraderingen, slik at GNOME Shell laster den omdøpte utvidelsen. Til da tar GNOME bildene med blits.
- Uten utvidelsen spør GNOME én gang til om tilgang til skjermbilder og om hurtigtasten, fordi appen har ny app-ID.
- Debian-pakken heter `agentic-screenshot` og erstatter `agentic-klipp`.
- Versjonsloggen i appen vises på engelsk for alle språk utenom norsk bokmål, som fortsatt får den norske.
- Start på nytt i Innstillinger lar ikke lenger appen være lukket når den gamle prosessen ikke blir ryddet bort med en gang. Den nye starter uansett etter fem sekunder.

## 1.3.0 — 2026-11-20

- Klipp finnes nå på ti språk: norsk bokmål, engelsk, svensk, dansk, tysk, fransk, spansk, italiensk, brasiliansk portugisisk og nederlandsk.
- Nytt språkvalg under Innstillinger → Utseende. Standard er systemspråket, og Klipp bruker engelsk når systemspråket ikke støttes.
- Knappen Start på nytt tar i bruk det nye språket med en gang.
- Programnavnet og hurtighandlingene i appoversikten er oversatt.
- Valgknappene i Innstillinger viser alltid hele teksten.
- Bildehylla mister ikke lenger historikk ved raske endringer, rett etter oppstart eller når bildemappen er frakoblet. En ødelagt hyllefil tas vare på som shelf.tsv.ugyldig.
- Frihåndsklipp er helt gjennomsiktige utenfor formen, og Kopier i tegnevinduet virker også etter at du har byttet app.
- Debian-pakken finner Klipp-utvidelsen, og Alt+K virker etter installasjon fra pakken.
- Kildekoden er åpen på GitHub med engelsk README, retningslinjer for bidrag og automatisk testing.
- README viser nye skjermbilder på engelsk.

## 1.2.1 — 2026-11-14

- Teksten i versjonsloggen kan ikke markeres eller få tekstfokus.
- Fjernet den blå tekstmarkeringen som kom automatisk øverst ved åpning.
- Fjernet innledningsteksten over versjonene.
- Nyeste versjon vises øverst, eldste nederst.
- README viser hovedvinduet, toppen av Innstillinger og annoteringsmodus.

## 1.2.0 — 2026-11-14

- Ny klikkbar Versjonslogg under Innstillinger → Om Klipp.
- Eget rullbart vindu med versjonene fra eldste til nyeste og gjeldende versjon og byggnummer.
- Samme innebygde versjonslogg i appen og på GitHub, også tilgjengelig uten internett.
- Felles versjon 1.2.0 for appen og de interne bibliotekene.
- Byggnummer kan fastsettes ved release; lokale bygg bruker tidspunktet i Europe/Oslo.
- Oppdatert README med tre skjermbilder og kreditering av Claude og Codex.
- Release-pakker med Linux-binær, installasjonsarkiv, Debian-pakke og SHA-256-kontrollsummer.

## 1.1.1 — 2026-11-08

Første taggede og publiserte GitHub-versjon. Oppsummerer også utviklingen i 1.1-serien før publisering.

- Skjermbilder av område, frihånd, vindu og hele skjermen, med Alt+K som standard.
- GNOME Shell-utvidelse for stille opptak og global hurtigtast.
- Hylle med flervalg, samlet kopiering og papirkurv.
- Tegneverktøy med blant annet pil, linje, ellipse, merkepenn, tekst, sladd og viskelær.
- Åtte farger, egen farge og fem størrelser i tegnevinduet.
- Minimer-knapp og signaturen Code Pro under Om Klipp.
- Installasjonsskript som sjekker avhengigheter, bygger, installerer og starter appen.

## 1.0.0 — Tidlig lokal versjon

Første lokale versjon, omtalt i konfigurasjonsmigreringen. Nøyaktig dato og full endringsliste er ikke bevart i Git-historikken.

- Den globale hurtigtasten var Super+Shift+K.
- Hylla var bred som standard, og opptak hadde et eget bekreftelsessteg.
