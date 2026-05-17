# Plan stratégique — Open Island : portage Windows, commercialisation, site web

## Context

Open Island Linux est aujourd'hui un overlay Tauri 2 + Svelte 5 fonctionnel sur Linux (KDE/XWayland) : pill flottante en haut de l'écran qui affiche le statut des agents Claude Code et permet d'approuver/refuser les tool calls en temps réel. Le produit Linux est quasi terminé (positionnement, design Nothing DS, permission flow).

L'objectif stratégique de ce plan : porter le produit sur Windows pour viser les **vibe coders** (devs indé/solo qui utilisent Cursor/Claude Code/Cline en autopilot), le commercialiser en one-time license, et lancer un site web minimal.

**Risque connu** : l'espace AI agent est complètement saturé. Un concurrent direct existe déjà sous le nom **AgentWard** (agentward.ai) qui décrit verbatim notre fonction ("permission enforcement for AI agents, approval gates, runtime control"). On doit se différencier par UX, design, et niche (vibe coders Claude Code spécifiquement), pas par positionnement seul.

---

## 1. Portage Windows (Phase 1)

### Charge estimée
**2-3 jours pour MVP fonctionnel**, +1-2 jours pour polish (installeur, tests). Le frontend Svelte est 100% portable, Tauri 2 supporte Windows nativement via WebView2. Seuls 3 endroits dans la couche Rust touchent du système Linux.

### Changements requis

| Composant | Linux actuel | Windows | Charge |
|---|---|---|---|
| **IPC bridge** | `tokio::net::UnixListener` (`src-tauri/src/bridge/server.rs:8`) | **TCP localhost** (port aléatoire dans `~/.config/open-island/port`) — plus simple que Named Pipes, évite les `#[cfg]` partout | 2-3h |
| **Détection barre tâches** | `kde_panel_thickness()` lit `~/.config/plasmashellrc` (`src-tauri/src/lib.rs:143`) | Win32 `SHAppBarMessage(ABM_GETTASKBARPOS)` | 2-3h |
| **GDK_BACKEND=x11** | hack XWayland dans `src-tauri/src/main.rs` | Gater derrière `#[cfg(target_os = "linux")]` | 5 min |
| **Path settings.json** | `~/.claude/settings.json` via `dirs::home_dir()` | `%USERPROFILE%\.claude\settings.json` — déjà cross-platform | 0 |
| **System tray** | déjà abstrait par Tauri | déjà OK | 0 |
| **Hook relay** (`hook-cli/src/main.rs`) | connecte au Unix socket | partage le module IPC refactoré | 1h |
| **Installeur** | .deb/.AppImage | MSI/NSIS via `tauri-bundler` | 1-2h |
| **Code signing** | optionnel | **SKIP pour v1** — SmartScreen warning acceptable, réputation se construit avec les téléchargements | 0 |

### Process recommandé
1. Refactor `BridgeListener`/`BridgeStream` en abstraction cross-platform sur Linux d'abord (sans casser l'existant)
2. Ajouter le code Windows derrière `#[cfg(target_os = "windows")]`
3. Tester sur Windows VM ou dual-boot
4. Bundler MSI via `tauri-bundler` (config dans `src-tauri/tauri.conf.json` → `bundle.targets`)
5. CI GitHub Actions avec runners `windows-latest` + `ubuntu-latest`

---

## 2. Commercialisation (Phase 2)

### Modèle de pricing : **one-time license $19**
- Modèle Sublime Text : achat unique avec 1 an d'updates inclus, ensuite l'app continue de marcher mais les nouvelles versions payantes
- Pas d'abonnement (les devs détestent ça pour des outils desktop)
- Trial 14 jours sans clé, ou modèle "buy whenever" sans nag

### Stack de monétisation
- **Paiement** : Stripe Checkout (frais ~3%) ou Paddle/LemonSqueezy si tu veux qu'ils gèrent la TVA EU/US/UK
- **Génération de licence** : Keygen.sh ($19/mois) OU self-hosted simple (SQLite + signature ed25519)
- **Activation offline-first** : clé contient signature ed25519 vérifiable hors-ligne
- **Auto-update** : Tauri updater natif (signatures Minisign)

### Distribution
- **Site direct** ⭐ (0% cut, juste frais Stripe)
- **GitHub Releases** : version free/trial téléchargeable
- **Microsoft Store** : en parallèle, 12% cut, MS signe pour toi (élimine le besoin de cert + ajoute trust)
- **Linux** : AUR/Flathub gratuit

### Pas de code signing pour v1
Économie : ~$60-200/an. Les users verront un SmartScreen warning au premier lancement, peuvent cliquer "Run anyway". Si retours négatifs, prendre un cert SSL.com OV à $59/an plus tard.

---

## 3. Site web (Phase 3)

### Stack : HTML simple
- Pas de framework (le user préfère du HTML direct)
- Tailwind CDN ou CSS custom selon préférence
- Look Nothing DS : monochrome, JetBrains Mono, dot-matrix glyphs réutilisés du produit
- Hébergement **Cloudflare Pages** (gratuit, push git auto-deploy)

### Structure du site (1 page principale + secondaires)
```
/                  Hero + démo vidéo + CTA "Buy $19" / "Download"
/download          Win / Mac (plus tard) / Linux + checksums
/docs              Install, setup hooks Claude Code, FAQ
/changelog         Release notes
```

### Hero critique
- **Vidéo 15-20s autoplay muted** : terminal lance Claude Code → pill apparaît en haut → tool call Bash → user clique Allow → ça déverrouille. Hosted en MP4 + WebM.
- Sous le hero : 3 features avec glyphs dot-matrix (cohérence avec le produit)
- Pricing card simple : 1 offre $19, lifetime, 1 an updates

### Domaine
- TBD avec le nom final
- Format `.app` (~$15/an, force HTTPS, vibe dev tool)

### Go-to-market sequence
1. **Soft launch** : Twitter/X, friends-and-family (50-100 users)
2. **Show HN** : "Show HN: [Name] — a permission gate for Claude Code" (mardi-jeudi 9h ET)
3. **Product Hunt** : prep hunter + GIFs
4. **Reddit** : r/ClaudeAI, r/LocalLLaMA, r/commandline (éviter r/programming)
5. **Anthropic DevRel** : pitch comme outil tiers améliorant Claude Code UX

---

## 4. Nom du produit : TBD

L'espace AI agent est totalement saturé. Toutes les métaphores buddy/watcher/guardian en anglais sont prises (Spotter/Wingman/Sidekick/Sentry/Aegis/Belay/Sherpa/Anchor/Ward/Vesper/Tomo tous conflictuels). Le concurrent direct **AgentWard** occupe déjà notre positionnement verbatim.

**Décision** : parker le nom. On peut launcher sous Open Island ou un nom temporaire et trancher juste avant le commercial launch (post-MVP Windows). Le nom n'est pas bloquant pour avancer techniquement.

**Direction confirmée pour la suite** : mot inventé (style Spotify/Vercel/Stripe) plutôt que métaphore. Candidats partiellement vérifiés : **Klari** (semble libre, à confirmer), **Iko** (à confirmer). Tous les mots empruntés à des langues existantes sont brûlés.

---

## 5. Roadmap (timeline)

| Phase | Durée | Output |
|---|---|---|
| **0. Finition Linux** | 1 sem | Pill positionnement OK sur eDP-1, AppImage, tests utilisateurs |
| **1. Refactor IPC cross-platform** | 1 jour | Abstraction `BridgeListener` TCP localhost, encore sur Linux |
| **2. Port Windows MVP** | 2-3 jours | Binaire Windows fonctionnel, taskbar detection, MSI bundle |
| **3. Site web v1** | 2-3 jours | HTML statique, hero vidéo, Cloudflare Pages |
| **4. Système de licence** | 3-4 jours | Stripe Checkout + Keygen (ou self-hosted), intégration app |
| **5. Décision finale du nom** | inline | Pendant la phase 3 ou 4, idéalement avec retour de friends-and-family |
| **6. Soft launch** | 1 sem | Friends-and-family, polish bugs |
| **7. Public launch** | 1 jour | Show HN + Product Hunt |

**Total avant launch public : ~3-4 semaines** (1 dev focus, sans imprévus).

---

## 6. Risques & questions ouvertes

- **Anthropic peut intégrer un permission UX natif** dans Claude Code → différencier par UX (multi-agent dashboard, historique, design Nothing DS), pas seulement la fonction
- **AgentWard est déjà sur le marché** avec le même positionnement → notre edge = design + niche vibe coders Claude Code + UX pill (eux sont CLI/config)
- **macOS** : Vibe Island existe déjà → clarifier la stratégie (skip, collaborer, ou clean room)
- **Cible vibe coders B2C vs équipes B2B** : pour l'instant cibler B2C $19 ; pivot équipes possible plus tard

---

## 7. Critical files (pour le portage)

- `src-tauri/src/bridge/server.rs:8` — `UnixListener` à abstraire en `BridgeListener` cross-platform (TCP localhost recommandé)
- `src-tauri/src/lib.rs:143` — `kde_panel_thickness()` à wrapper avec implémentation Windows via Win32 `SHAppBarMessage`
- `src-tauri/src/main.rs` — `GDK_BACKEND=x11` à gater par `#[cfg(target_os = "linux")]`
- `src-tauri/src/hooks/claude.rs` — vérifier que `dirs::home_dir()` est utilisé partout (cross-platform OK)
- `hook-cli/src/main.rs` — relais à adapter au nouveau transport IPC
- `src-tauri/tauri.conf.json` — ajouter `bundle.targets` Windows (msi, nsis)
- **Nouveau** : `open-island-linux/STRATEGY.md` — copier ce plan dans le projet après ExitPlanMode

---

## 8. Vérification end-to-end (post-portage Windows)

1. `cargo tauri build --target x86_64-pc-windows-msvc` produit un MSI
2. Installer le MSI sur Windows 11, lancer, vérifier que la pill se positionne sous la taskbar (peu importe sa position : top/bottom/left/right)
3. Lancer Claude Code dans un terminal Windows, déclencher un hook Bash → permission s'affiche dans la pill
4. Cliquer Allow → commande s'exécute. Cliquer Deny → bloqué avec message dans le terminal.
5. Multi-session : ouvrir 2 Claude Code en parallèle → 2 lignes dans la pill
6. Tester auto-update Tauri
7. Tester désinstallation propre (registre + fichiers + hooks `%USERPROFILE%\.claude\settings.json` nettoyés)
