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
   │      │                    TomlSettingsRepository
   │      ▼
   ├── application             cas d'usage (Browser) + ports :
   │      │                    WebEngine, EngineEvent, SettingsRepository
   │      ▼
   └── domain                  règles métier pures : WebAddress, omnibox,
                               History, Tab, Session, Settings
```

| Couche | Crate | Dépend de | Rôle |
|---|---|---|---|
| Domaine | `crates/domain` | `url` | Entités et règles, sans I/O |
| Application | `crates/application` | domain | Cas d'usage, ports (traits) |
| Infrastructure | `crates/infrastructure` | domain, application, wry, toml | Implémente les ports |
| Présentation | `src/` | toutes | UI egui, câblage |

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

## Icône

Même licorne que codingUnicorns et storingUnicorns, le sabot posé sur un
globe. Pour la régénérer (Pillow requis) :

```powershell
python scripts/draw-icon.py assets/icon.png 256
./scripts/New-Icon.ps1   # assets/icon.ico, intégrée à l'exe par build.rs
```

## Pistes

- Historique et favoris persistants, restauration de session
- Page de réglages dans l'interface
- Navigation privée, bloqueur de contenu
- Téléchargements
- Mise à jour automatique et paquets (MSI, DMG, AppImage) comme
  codingUnicorns / storingUnicorns
