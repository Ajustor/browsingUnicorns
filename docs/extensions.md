# Extensions : du natif Windows à un moteur d'extensions à nous

## Où on en est

Sur Windows, les extensions Chrome tournent dans WebView2 lui-même :

- installation depuis un lien du Chrome Web Store, un id ou un `.crx`
  (`crates/infrastructure/src/extensions`) : le `.crx` est décompressé dans
  `<profil>/extensions/<id>/` et la clé publique de l'éditeur est écrite dans
  le `manifest.json`, pour garder l'id du Web Store ;
- `WryEngine` aligne le profil WebView2 sur la liste de l'utilisateur
  (`webview2_extensions.rs`) : ajout, activation, suppression. Les extensions
  intégrées à WebView2 (lecteur PDF…) ne sont jamais touchées ;
- vérifié avec uBlock Origin Lite : les traceurs (Google Analytics,
  DoubleClick, Facebook) sont bloqués quand l'extension est active.

Limites, qui viennent de WebView2 :

- **pas d'interface d'extension** : ni bouton, ni badge, ni popup attaché à
  l'onglet. Le popup s'ouvre dans un onglet, mais il y voit cet onglet comme
  « l'onglet actif » et pas la page visitée (uBO Lite y affiche « pas un site
  Web ») ; les pages d'options, elles, marchent bien ;
- Microsoft ne publie pas la liste des API supportées ;
- **rien sur macOS, Linux ni Android** : WKWebView, WebKitGTK et la WebView
  Android n'ont aucun moteur d'extensions Chrome.

## L'idée : convertir les extensions vers un moteur à nous

Une extension WebExtension, c'est surtout quatre choses, et chacune a un
équivalent dans **toutes** les web views qu'on utilise :

| Ce que fait l'extension | WebView2 (Windows) | WKWebView (macOS) | WebKitGTK (Linux) | WebView (Android) |
|---|---|---|---|---|
| Bloquer des requêtes (`declarativeNetRequest`) | `WebResourceRequested` | `WKContentRuleList` | `WebKitUserContentFilter` | `shouldInterceptRequest` |
| Scripts / CSS dans les pages (`content_scripts`) | `AddScriptToExecuteOnDocumentCreated` | `WKUserScript` | `WebKitUserScript` / `UserStyleSheet` | `addDocumentStartJavaScript` (androidx.webkit) |
| Parler à la page (`runtime.sendMessage`…) | `postMessage` / IPC wry | `WKScriptMessageHandler` | `script-message-received` | `WebMessageListener` |
| Logique de fond (service worker), popup, options | web view cachée | web view cachée | web view cachée | WebView cachée |

Point clé : **WebKit (macOS et Linux) lit le format des « content blockers »
de Safari**, et les règles `declarativeNetRequest` des extensions MV3 s'y
convertissent presque une à une. C'est exactement ce que fait Safari pour
porter les bloqueurs Chrome.

On n'émule donc pas Chrome : à l'installation, on **convertit** l'extension
vers un format interne (« Unicorn extension »), que chaque moteur sait
exécuter avec ses propres outils.

### Le pipeline d'installation

```
.crx / Web Store ─► décompression ─► analyse du manifest (MV2 / MV3)
                                         │
                ┌────────────────────────┼─────────────────────────┐
                ▼                        ▼                         ▼
     règles réseau (DNR)       content scripts / CSS       fond, popup, options
     → règles normalisées      → règles d'injection        → pages hébergées +
       (domaine Rust)            (matches, run_at,           polyfill `chrome.*`
                                  all_frames)
                └────────────────────────┼─────────────────────────┘
                                         ▼
                         rapport de compatibilité
            (complète / partielle / non supportée, API manquantes)
```

1. **Règles réseau** : les rulesets DNR (JSON) deviennent des règles du
   domaine (`action`, motif d'URL, types de ressources, domaines inclus /
   exclus). Chaque moteur les compile :
   - WebKit : JSON Safari, compilé une fois par WebKit (rapide) ;
   - WebView2 / Android : un *matcher* Rust partagé (index par domaine, motifs
     façon Adblock), appelé pour chaque requête.
2. **Content scripts** : injectés au bon moment (`document_start`, `_end`,
   `_idle`) dans les pages dont l'URL correspond, CSS compris.
3. **Polyfill `chrome.*` / `browser.*`** : un script injecté avant ceux de
   l'extension, qui implémente les API en relayant vers le cœur Rust :
   - `storage.local` / `storage.sync` → stockage du profil, et donc
     **synchronisable** avec notre relais chiffré ;
   - `runtime.sendMessage` / `onMessage`, `runtime.getURL` ;
   - `tabs.query` / `tabs.sendMessage` / `tabs.create` → le cas d'usage
     `Browser` (c'est lui qui connaît les onglets) ;
   - `i18n`, `alarms`, `declarativeNetRequest.updateDynamicRules`…
4. **Fond, popup, options** : chaque extension a une web view cachée qui
   charge son `background` (MV2) ou son service worker (MV3, exécuté comme un
   script de page). Les fichiers sont servis par un protocole à nous
   (`bu-ext://<id>/…`), puisque `chrome-extension://` n'existe pas hors de
   Chromium. Le popup s'affiche dans le panneau latéral, et le polyfill lui
   donne **le vrai onglet actif** : ce qui ne marche pas aujourd'hui sous
   WebView2 marcherait alors partout.
5. **Rapport de compatibilité** : on cherche les API utilisées (permissions du
   manifest, appels `chrome.xxx` dans le code) et on affiche avant
   l'installation ce qui marchera.

### Ce qui ne pourra pas marcher

- `webRequest` bloquant (extensions MV2 comme uBlock Origin classique) :
  possible seulement là où on intercepte les requêtes (WebView2, Android), pas
  sur WebKit ;
- `devtools`, `debugger`, `nativeMessaging`, `identity`, `proxy`,
  `downloads` avancé : pas d'équivalent dans les web views ;
- les extensions qui dépendent du rendu exact de l'interface de Chrome.

En pratique, les bloqueurs de contenu MV3, les extensions « de page » (mode
sombre, traducteur, gestion des cookies, Stylus, scripts utilisateur) et la
plupart des petits utilitaires passent ; les gros outils intégrés au
navigateur, non.

## Architecture proposée

```
domain        UnicornExtension (manifest normalisé), NetworkRule, InjectionRule,
              CompatibilityReport ; conversion DNR → règles, matcher partagé
application   ExtensionRuntime : installe, active, route les messages
              port ExtensionHost (par moteur) : set_network_rules,
              set_injections, open_hidden_page, post_to_page
infrastructure
  windows     WebView2 natif (actuel) OU ExtensionHost générique
  macos/linux ExtensionHost WebKit (content rule lists + user scripts)
  android     ExtensionHost Kotlin (shouldInterceptRequest + WebMessageListener),
              le reste en Rust via UniFFI comme aujourd'hui
```

Le même cœur Rust sert desktop et Android, comme pour la synchro. La liste
des extensions et leur `storage` se synchronisent avec le relais existant.

Sur Windows, deux possibilités : garder WebView2 natif (fidélité maximale) ou
passer aussi par notre moteur (même comportement partout, popups dans le
panneau, synchro des réglages d'extension). Je propose de garder le natif par
défaut et d'utiliser notre moteur pour les extensions que WebView2 ne sait pas
afficher correctement.

## Étapes

1. ✅ **Règles réseau partout** (`domain::content_rules`,
   `infrastructure::extensions::{dnr, safari}`, `webkit_rules.rs`,
   `crates/mobile/src/extensions.rs`) : moteur de règles commun (index par
   domaine et par mot du filtre, ~0,1 ms par requête avec les 18 893 règles
   de uBlock Origin Lite), conversion vers WebKit (116 095 règles), Android
   via `shouldInterceptRequest`. Limites : pas de `regexFilter` ni de
   `modifyHeaders` ; sur WebKit, pas de redirection vers les scripts de
   remplacement (ces règles sont ignorées pour ne pas casser les pages).
2. ✅ **Content scripts + CSS + `storage` + scripts utilisateur**
   (`domain::page_scripts`, `infrastructure::{page_scripts, page_injection,
   script_storage, user_scripts}`) : chaque script est injecté à part au
   début du document, dans toutes les frames, avec sa copie d'un petit
   runtime (motifs d'URL, `run_at`, `chrome.storage` / `i18n` / `runtime`,
   `GM_*` / `GM.*`). Le stockage passe par Rust (IPC de wry, `BUBridge` sur
   Android), protégé par un jeton tiré à chaque lancement et un canal
   aléatoire par script. Limites : pas de monde isolé (les scripts partagent
   le monde de la page), `runtime.sendMessage` sans réponse tant qu'il n'y a
   pas de page de fond, `runtime.getURL` sans fichiers servis hors Windows,
   pas de stockage depuis les iframes sous Windows.
3. ✅ **Fond et popup/options** (`extension_server`, `extension_hub`,
   `extensions::state`) : fichiers servis sur l'origine de chaque extension
   (`bu-ext://<id>/`, `http://bu-ext.<id>/` sous Windows,
   `https://<id>.bu-ext.invalid/` sur Android), le runtime n'étant donné
   qu'aux pages que le serveur vient de servir (nonce à usage unique) ; page
   de fond dans une vue cachée (service worker exécuté comme une page,
   `importScripts` synchrone) ; un hub côté Rust route les messages et répond
   à `tabs`, `scripting`, `declarativeNetRequest` (jeux de règles et règles
   dynamiques gardés dans le profil et appliqués au blocage), diffuse
   `storage.onChanged` et `tabs.on*`. Le popup s'affiche dans le panneau
   latéral et voit le vrai onglet actif. Limites : pas de `runtime.connect`
   (ports), pas d'`offscreen`, alarmes perdues au redémarrage, pas de badge ni
   de menu contextuel, messages des iframes non routés sous Windows.
4. **Rapport de compatibilité** à l'installation, **synchro** des extensions
   et de leurs réglages.
5. Mises à jour automatiques des extensions (requête au Web Store comme
   Chrome, une fois par jour).
