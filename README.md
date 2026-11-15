# 🦄 Browsing Unicorns

Un navigateur web léger et performant construit en Rust.

**Philosophie** : ne pas embarquer de moteur. Les pages sont rendues par la
WebView du système (WebView2 sur Windows, WKWebView sur macOS, WebKitGTK sur
Linux) ; l'interface (onglets, barre d'adresse) est dessinée avec egui. Le
binaire reste petit et le moteur profite des mises à jour de sécurité de l'OS.

---

## Fonctionnalités

- 🗂️ Onglets : ouverture (Ctrl+T, bouton `+`), fermeture (Ctrl+W, `×`, clic
  molette), passage d'un onglet à l'autre (Ctrl+Tab / Ctrl+Shift+Tab)
- 🔎 Barre d'adresse unique : une adresse complète, un nom d'hôte
  (`rust-lang.org`, `localhost:3000`) ou une recherche (Ctrl+L / F6)
- ⬅️ Précédent / suivant (Alt+←/→), recharger (F5 / Ctrl+R), accueil (Alt+Home)
- 🪟 Les liens `target="_blank"` et `window.open` s'ouvrent dans un nouvel onglet
- 🔒 Indicateur HTTPS, titre de la page dans l'onglet et la fenêtre
- ⭐ Favoris : étoile dans la barre d'adresse (Ctrl+D), barre de favoris sous
  la barre d'outils (clic molette : nouvel onglet)
- 🕘 Historique avec recherche (Ctrl+H) ; favoris (Ctrl+Shift+O) dans le même
  panneau latéral
- ♻️ Les onglets ouverts sont rouverts au démarrage
- 🔄 Synchro chiffrée de bout en bout avec les autres appareils (favoris,
  historique, onglets ouverts, réglages) : onglet *Sync* du panneau latéral,
  appairage par code ou QR code, relais Cloudflare (`relay/sync`) qui ne voit
  que des données chiffrées
- 🧩 Extensions Chrome : onglet *Extensions* du panneau latéral (menu
  *Extensions* sur Android), installation par lien du Chrome Web Store, id ou
  fichier `.crx`, activation, suppression.
  - Windows : elles tournent dans WebView2 ; popup et options s'ouvrent dans
    un onglet (WebView2 n'affiche pas de bouton d'extension).
  - Android, macOS, Linux : le navigateur applique leurs règles de blocage
    (`declarativeNetRequest`) : bloqueurs de pub et de pistage. Sur WebKit
    elles sont converties au format des *content blockers* de Safari.
  - Android, macOS, Linux : leurs scripts de page (`content_scripts`) tournent
    aussi, avec `chrome.storage`, `chrome.i18n` et `chrome.runtime`.
  - La suite (un moteur d'extensions complet pour toutes les plateformes) est
    décrite dans [`docs/extensions.md`](docs/extensions.md)
- 📜 Scripts utilisateur (Greasemonkey / Tampermonkey) partout : adresse d'un
  `.user.js` dans l'onglet *Extensions*, `GM_*` et `GM.*` (valeurs gardées
  dans le profil)
- 💾 Profil dans `<data>/browsing-unicorns/` (`history.json`,
  `bookmarks.json`, `session.json`, `extensions.json`, `webview/` pour les
  données des pages), écrit au plus toutes les 5 s et à la fermeture, par
  remplacement atomique. `BU_PROFILE_DIR` fait pointer l'appli vers un autre
  profil (tests)
- ⚙️ Page d'accueil et moteur de recherche réglables dans
  `<config>/browsing-unicorns/settings.toml`

```toml
home_page = "https://duckduckgo.com/"

[search_engine]
name = "DuckDuckGo"
template = "https://duckduckgo.com/?q={query}"
```

> Les raccourcis ne sont reçus que lorsque l'interface a le focus : une page
> qui a le focus clavier garde ses touches.

## Architecture

Clean architecture en workspace Cargo ; les dépendances pointent vers le
domaine, jamais l'inverse.

```
browsing-unicorns (src/)      présentation egui + composition root (bootstrap.rs)
   │
   ├── infrastructure          adaptateurs : WryEngine (WebView système),
   │      │                    TomlSettingsRepository, JsonRepository, SystemClock
   │      ▼
   ├── application             cas d'usage (Browser) + ports :
   │      │                    WebEngine, EngineEvent, Repository<T>, Clock
   │      ▼
   └── domain                  règles métier pures : WebAddress, omnibox,
                               History, Tab, Session, BrowsingHistory,
                               Bookmarks, Settings
```

| Couche | Crate | Dépend de | Rôle |
|---|---|---|---|
| Domaine | `crates/domain` | `url` | Entités et règles, sans I/O |
| Application | `crates/application` | domain | Cas d'usage, ports (traits) |
| Infrastructure | `crates/infrastructure` | domain, application, wry, toml | Implémente les ports |
| Présentation | `src/` | toutes | UI egui, câblage |
| Mise à jour | `crates/updater` | ureq, sha2, self-replace | Autonome, repris de storingUnicorns |

- L'UI ne parle qu'au cas d'usage `Browser` : les widgets produisent des
  `Command`, exécutées une fois par frame.
- Le moteur remonte ce qui se passe dans les pages (`EngineEvent` : début /
  fin de chargement, titre, demande de nouvel onglet) par un canal, vidé à
  chaque frame.
- L'historique précédent/suivant est géré par le moteur ; le domaine en garde
  un miroir pour activer les boutons.
- Changer de moteur (Servo, CEF…) revient à écrire un autre `WebEngine`.

## Compiler et lancer

```bash
cargo run              # debug (devtools activés dans les pages)
cargo run --release    # optimisé pour la taille (LTO, strip)
cargo test --workspace
```

Prérequis :

- **Windows** : runtime WebView2 (installé avec Windows 10/11).
- **macOS** : rien.
- **Linux** (X11) : `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev`.

## Android

L'app Android (`android/`, Kotlin + Jetpack Compose) réutilise les couches
domaine et application telles quelles : `crates/mobile` les expose à Kotlin avec
UniFFI. L'app fournit le moteur (une WebView Android par onglet, interface
`WebViewHost`) et le client HTTP de la synchro (`SyncHttp`) ; le chiffrement et
la fusion restent en Rust.

```powershell
.\scripts\build-android.ps1                # APK de debug
.\scripts\build-android.ps1 installDebug   # sur le téléphone branché (débogage USB)
```

(`scripts/build-android.sh` sous Linux / macOS.) Prérequis : SDK et NDK Android,
JDK 17+, `rustup target add aarch64-linux-android x86_64-linux-android`,
`cargo install cargo-ndk`. `-PsyncRelay=http://10.0.2.2:8787` (Gradle) fait
pointer l'app vers un relais local depuis l'émulateur.

## Icône

Même licorne que codingUnicorns et storingUnicorns, le sabot posé sur un
globe. Pour la régénérer (Pillow requis) :

```powershell
python scripts/draw-icon.py assets/icon.png 256
./scripts/New-Icon.ps1   # assets/icon.ico, intégrée à l'exe par build.rs
```

## Releases et mises à jour

Comme storingUnicorns :

1. Monter `version` dans `Cargo.toml` et ajouter sa section
   `## [X.Y.Z] - date` dans `CHANGELOG.md`.
2. Pousser un tag `vX.Y.Z` : `.github/workflows/release.yml` vérifie que le tag
   correspond à `Cargo.toml`, teste, construit `.msi`, `.exe` portable,
   AppImage, `.dmg` et les binaires seuls, crée la release GitHub puis publie
   sur GitHub Pages `latest.json` (version, notes, URL et SHA-256 de chaque
   fichier), la page de téléchargement (`pages/index.html`) et les scripts
   d'installation.
3. L'appli lit `https://ajustor.github.io/browsingUnicorns/latest.json` une
   fois par jour (`crates/updater`), propose la mise à jour dans un bandeau,
   vérifie le SHA-256, puis remplace l'exécutable (ou lance le `.msi` à la
   fermeture pour une installation dans Program Files). « Check for updates »
   dans l'onglet About du panneau latéral force la vérification.

Le dépôt est privé : GitHub Pages (public) est publié par Actions, ce qui
demande un compte Pro et Pages réglé sur « GitHub Actions ».

## Pistes

- Page de réglages dans l'interface
- Navigation privée, bloqueur de contenu
- Téléchargements
