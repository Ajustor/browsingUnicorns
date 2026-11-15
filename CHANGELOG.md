# Changelog

Toutes les évolutions notables de browsingUnicorns. Format :
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), versions
[SemVer](https://semver.org/lang/fr/).

## [Unreleased]

### Ajouté

- Les extensions qui ont un popup ont un bouton à côté de l'étoile des
  favoris ; leur popup s'ouvre dessous, à la taille de son contenu, voit
  l'onglet actif et se ferme d'un clic ailleurs.

### Modifié

- Sous Windows aussi, le navigateur fait tourner les extensions lui-même (le
  blocage passe par WebView2), ce qui permet leurs popups.
  `BU_EXTENSIONS=native` revient au moteur de WebView2.

### Corrigé

- Une entrée de l'historique occupait toute la hauteur du panneau.

## [0.4.0] - 2026-11-15

### Ajouté

- Pages de fond des extensions (service worker MV3 ou scripts MV2) sur
  Android, macOS et Linux, avec les messages entre scripts de page, page de
  fond et popup (`runtime.sendMessage`, `tabs.sendMessage`), `tabs.*`,
  `scripting.*` (`executeScript`, `insertCSS`, `registerContentScripts`),
  `declarativeNetRequest` (jeux de règles, règles dynamiques et de session),
  `alarms`, `storage.onChanged`, `tabs.on*`, `runtime.onInstalled`.
- Popup des extensions dans le panneau latéral (une fenêtre sur Android), qui
  voit l'onglet actif ; pages d'options dans un onglet. Les fichiers des
  extensions sont servis sur leur propre origine (`runtime.getURL`).
- `BU_EXTENSIONS=builtin` : sous Windows, faire tourner les extensions par le
  navigateur au lieu de WebView2 (pour essayer ce que font les autres
  plateformes).

## [0.3.2] - 2026-11-15

### Ajouté

- Scripts utilisateur (format Greasemonkey / Tampermonkey) sur toutes les
  plateformes : installation depuis l'adresse d'un `.user.js` (Greasy Fork…)
  dans l'onglet *Extensions*, `@match` / `@include` / `@exclude`, `@run-at`,
  `@require`, `GM_addStyle`, `GM_getValue` / `GM_setValue` (gardés dans le
  profil), `GM.*`, `GM_xmlhttpRequest` (limité par CORS).
- Scripts de page des extensions (`content_scripts` : JS, CSS, `run_at`,
  `all_frames`) sur Android, macOS et Linux, avec `chrome.storage`,
  `chrome.i18n` et `chrome.runtime` ; sous Windows, WebView2 les exécute
  lui-même.

### Corrigé

- Les scripts injectés manquaient la première page d'un nouvel onglet.

## [0.3.1] - 2026-11-15

### Ajouté

- Bloqueurs de pub et de pistage (uBlock Origin Lite, AdGuard…) sur Android,
  macOS et Linux : le navigateur applique lui-même les règles réseau
  (`declarativeNetRequest`) des extensions installées. Écran *Extensions* dans
  le menu de l'app Android. Les autres fonctions des extensions ne tournent
  que sur Windows.

## [0.3.0] - 2026-11-15

### Ajouté

- Extensions Chrome sur Windows : onglet *Extensions* du panneau latéral.
  Installation depuis un lien du Chrome Web Store, un id ou un fichier `.crx`,
  activation / désactivation, suppression ; popup et page d'options dans un
  onglet. Testé avec uBlock Origin Lite.
- `BU_PROFILE_DIR` : utiliser un autre dossier de profil.

### Corrigé

- Le panneau de l'historique s'élargissait à chaque mouvement de la souris.

## [0.2.1] - 2026-11-15

### Corrigé

- L'application installée par le `.msi` ne démarrait pas : WebView2 voulait
  écrire ses données (cookies, cache) dans Program Files. Elles sont maintenant
  dans le profil, `<data>/browsing-unicorns/webview`.

## [0.2.0] - 2026-11-15

### Ajouté

- Application Android (Kotlin + Jetpack Compose) sur le même cœur Rust que la
  version de bureau : onglets, barre d'adresse, historique, favoris, ouverture
  des liens venant des autres apps. L'APK est publié avec les autres fichiers.
- Synchro chiffrée de bout en bout entre appareils (favoris, historique,
  onglets ouverts, réglages) : onglet *Sync* du panneau latéral, appairage par
  code ou QR code. Les onglets des autres appareils peuvent être rouverts.
- Relais de synchro sur Cloudflare Workers (`relay/sync`) : il ne stocke que
  des données chiffrées, effacées après 180 jours sans mise à jour.

## [0.1.0] - 2026-11-15

Première version.

### Ajouté

- Navigation par onglets : nouvel onglet (Ctrl+T), fermeture (Ctrl+W, clic
  molette), passage de l'un à l'autre (Ctrl+Tab). Les liens « nouvelle
  fenêtre » s'ouvrent dans un onglet.
- Barre d'adresse : adresse complète, nom de site ou recherche (DuckDuckGo par
  défaut), Ctrl+L ; précédent / suivant, recharger, accueil ; cadenas HTTPS.
- Favoris : étoile dans la barre d'adresse (Ctrl+D) et barre de favoris.
- Historique avec recherche (Ctrl+H), effaçable.
- Les onglets ouverts sont rouverts au démarrage.
- `browsingUnicorns <adresse>` ouvre l'adresse dans un nouvel onglet.
- Mises à jour automatiques : vérification quotidienne, bandeau pour installer,
  remettre à plus tard ou ignorer une version ; « Check for updates » dans
  l'onglet About du panneau latéral.
- Installateur Windows (.msi), version portable (.exe), AppImage Linux et image
  disque macOS (.dmg).
