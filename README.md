# 🦄 Browsing Unicorns

Un navigateur web léger et performant construit en Rust.

**Philosophie** : ne pas embarquer de moteur. Les pages sont rendues par la
WebView du système (WebView2 sur Windows, WKWebView sur macOS, WebKitGTK sur
Linux) ; l'interface (onglets, barre d'adresse) est dessinée avec egui. Le
binaire reste petit et le moteur profite des mises à jour de sécurité de l'OS.

---

## Fonctionnalités

- 🗂️ Onglets : ouverture (Ctrl+T, bouton `+`) en dernière position,
  fermeture (Ctrl+W, `×`, clic molette), passage d'un onglet à l'autre
  (Ctrl+Tab / Ctrl+Shift+Tab), réorganisation par glisser-déposer (appui
  long puis glisser sur Android) ; glissé hors de la fenêtre, un onglet
  part dans une fenêtre à lui. Un lien ouvert dans un nouvel onglet
  s'ouvre en arrière-plan, juste après sa page, qui reste affichée
- 🪟 La barre d'onglets sert de barre de titre (boutons de fenêtre, double-clic
  pour agrandir, redimensionnement par les bords)
- 🔎 Barre d'adresse unique : une adresse complète, un nom d'hôte
  (`rust-lang.org`, `localhost:3000`) ou une recherche (Ctrl+L / F6). En
  tapant, elle propose les recherches déjà faites et les pages de
  l'historique (↑/↓ puis Entrée, ou un clic)
- ⬅️ Précédent / suivant (Alt+←/→), recharger (F5 / Ctrl+R), accueil (Alt+Home)
- 🛠️ Outils de développement de l'onglet actif : F12, Ctrl+Shift+I
  (Cmd+Option+I sur macOS) ou le bouton `</>`, dans leur propre fenêtre ;
  *Inspect* dans l'onglet *Extensions* pour la page de fond d'une extension. Android : *Web debugging* dans le menu, puis
  `chrome://inspect` sur un ordinateur
- 🪟 Les liens `target="_blank"` et `window.open` s'ouvrent dans un nouvel onglet
- 🦄 Look « licorne » : dégradé violet → cyan derrière les onglets, barres
  nuit ; panneaux en thème clair ou sombre selon le système ; police de
  l'interface du système. Sur Android, onglets en grille avec aperçus
- 🔒 Indicateur HTTPS ; un clic sur le cadenas montre la connexion (protocole,
  chiffrement) et la chaîne de certificats du site (Windows, Android). Titre
  de la page dans l'onglet et la fenêtre
- 🛡️ Bloqueur de pubs et de traqueurs intégré (EasyList, EasyPrivacy, filtres
  et Quick fixes d'uBlock, avec le moteur de Brave `adblock-rust`), sur
  ordinateur et Android : requêtes bloquées, emplacements de pub masqués sur
  les sites que les listes nomment, et les scriptlets d'uBlock Origin (GPL,
  téléchargés depuis la pré-version `ublock-resources` du projet, convertis
  chaque jour) qui retirent les pubs de YouTube, et sur Twitch le script
  *vaft* de TwitchAdSolutions (MIT, la suite maintenue par ryanbr,
  téléchargée avec les listes) qui remplace les coupures par un flux sans
  pub ; sur Netflix, un script maison coupe le son des pubs et les accélère,
  et ferme la pub de la pause. Listes téléchargées au premier lancement puis vérifiées
  chaque jour, rangées dans `<data>/browsing-unicorns/blocker/`. Bouclier à côté du
  cadenas (dans la barre d'adresse sur Android) : nombre d'éléments bloqués
  sur la page (pas sous macOS et Linux, où WebKit ne le dit pas), blocage
  coupé pour un site ou partout ; réglages `blocker` et `blocker_allowlist`
  synchronisés avec les autres
- ⬇️ Téléchargements dans le dossier Téléchargements, un autre dossier, ou
  là où on le demande à chaque fois (*Settings*) : la barre du bas montre
  leur progression, les annule, les ouvre ; sur Android, le gestionnaire
  de téléchargement du système
- ⭐ Favoris : étoile dans la barre d'adresse (Ctrl+D), dossiers, barre de
  favoris sous la barre d'outils (clic molette : nouvel onglet) ; import
  depuis Chrome, Edge, Brave, Vivaldi ou un fichier HTML exporté
- ⚙️ *Settings* : page d'accueil, moteur de recherche, téléchargements,
  mots de passe, zoom par site, effacer les données de navigation
- ↩️ Rouvrir le dernier onglet fermé (Ctrl+Shift+T, clic droit sur `+`)
- 🎬 Vidéos en plein écran sur tout l'écran ; icône des sites et
  haut-parleur (clic : couper le son) sur les onglets
- 🕘 Historique avec recherche (Ctrl+H) ; favoris (Ctrl+Shift+O) dans le même
  panneau latéral
- ♻️ Les onglets ouverts sont rouverts au démarrage
- 🔄 Synchro chiffrée de bout en bout avec les autres appareils (favoris,
  historique, onglets ouverts, réglages) : onglet *Sync* du panneau latéral,
  appairage par code ou QR code, relais Cloudflare (`relay/sync`) qui ne voit
  que des données chiffrées
- 🧩 Extensions Chrome : onglet *Extensions* du panneau latéral (menu
  *Extensions* sur Android), installation par lien du Chrome Web Store, id ou
  fichier `.crx`, activation, suppression. Sur ordinateur, *Choose .crx…*
  et *Load unpacked…* installent un paquet ou le dossier d'une extension
  décompressée (le recharger reprend ses modifications). Messagerie native
  vers les programmes installés (Bitwarden Desktop, KeePassXC…) sur
  ordinateur. Sur Android, la page
  d'une extension du Web Store propose de l'installer.
  - Le navigateur les fait tourner lui-même sur toutes les plateformes :
    règles de blocage (`declarativeNetRequest`, converties au format des
    *content blockers* de Safari sur WebKit, appliquées par
    `WebResourceRequested` sous WebView2), scripts de page, page de fond.
  - Un bouton par extension à popup, à côté de l'étoile des favoris ; le
    popup s'ouvre dessous et voit l'onglet actif.
  - Le navigateur fournit une grande partie des API `chrome.*` (`storage`,
    `runtime`, `tabs`, `scripting`, `declarativeNetRequest`, `alarms`,
    `i18n`…). Sous Windows, `BU_EXTENSIONS=native` confie les extensions au
    moteur de WebView2 à la place (pas de popup à côté de la page).
  - La suite (un moteur d'extensions complet pour toutes les plateformes) est
    décrite dans [`docs/extensions.md`](docs/extensions.md)
- 📜 Scripts utilisateur (Greasemonkey / Tampermonkey) partout : adresse d'un
  `.user.js` dans l'onglet *Extensions*, `GM_*` et `GM.*` (valeurs gardées
  dans le profil)
- 👥 Profils (travail, perso…) : chacun a son historique, ses favoris, ses
  onglets, ses extensions, ses cookies, ses réglages et sa synchro, et s'ouvre
  dans sa propre fenêtre. Avatar à droite de la barre d'outils → onglet
  *Profiles* du panneau latéral (ajouter, ouvrir, renommer, couleur,
  supprimer). `browsingUnicorns --profile <id>` ouvre un profil directement.
- 🕶️ Navigation privée : sur ordinateur, une fenêtre privée (Ctrl+Shift+N,
  bouton masque à gauche de l'avatar, `--private`) en gris anthracite ; ni
  historique ni onglets restaurés, cookies et stockage des sites dans un
  dossier temporaire supprimé à la fermeture, réglages changés en mémoire
  seulement, favoris en lecture seule, ni extensions, ni scripts
  utilisateur, ni synchro, ni vérification des mises à jour. Sur Android,
  des onglets privés (bascule *Normal / Private* de l'écran des onglets) dans
  un profil WebView à part, supprimé avec le dernier d'entre eux ; absents
  si la WebView est trop ancienne
- 💾 Profil par défaut dans `<data>/browsing-unicorns/` (`history.json`,
  `bookmarks.json`, `session.json`, `extensions.json`, `webview/` pour les
  données des pages), les autres dans `profiles/<id>/` en dessous, la liste
  dans `profiles.json`. Écrit au plus toutes les 5 s et à la fermeture, par
  remplacement atomique. `BU_PROFILE_DIR` fait pointer l'appli vers un autre
  dossier (tests)
- ⚙️ Page d'accueil et moteur de recherche réglables dans
  `<config>/browsing-unicorns/settings.toml` (profil par défaut) ou
  `profiles/<id>/settings.toml`

```toml
home_page = "https://duckduckgo.com/"

[search_engine]
name = "DuckDuckGo"
template = "https://duckduckgo.com/?q={query}"
```

> Sous Windows, les raccourcis marchent aussi quand une page a le focus.
> Ailleurs, une page qui a le focus clavier garde ses touches.

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
4. Android fait de même (`AppUpdates.kt`) : bandeau, téléchargement de
   `browsingUnicorns-android.apk`, SHA-256, puis l'installateur d'Android
   (même clé de signature : c'est une mise à jour). *Check for updates* dans
   le menu.

Le dépôt est privé : GitHub Pages (public) est publié par Actions, ce qui
demande un compte Pro et Pages réglé sur « GitHub Actions ».

## Pistes

- Page de réglages dans l'interface
- Masquage cosmétique générique du bloqueur de pubs
