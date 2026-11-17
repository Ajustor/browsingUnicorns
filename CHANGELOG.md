# Changelog

Toutes les évolutions notables de browsingUnicorns. Format :
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), versions
[SemVer](https://semver.org/lang/fr/).

## [Unreleased]

### Corrigé

- Impossible de se connecter à Amazon (et aux sites qui proposent une
  passkey) avec Bitwarden : Bitwarden ouvrait sa fenêtre de passkey, puis
  attendait sa fermeture pour rendre la main au site, mais le navigateur
  n'avait pas de vraies fenêtres (`windows.create` ouvrait un onglet
  anonyme, `windows.onRemoved` n'arrivait jamais). Un onglet ouvert par
  `windows.create` est désormais une fenêtre pour les extensions : son
  identifiant et son type, `windows.get`/`update`/`remove`, et
  `windows.onRemoved` quand il se ferme. Fermer la fenêtre de Bitwarden
  rend donc la connexion au site.

## [0.12.2] - 2026-11-16

### Corrigé

- Le remplissage automatique de Bitwarden ne marchait pas, pour deux
  raisons :
  - les scripts qu'une extension injecte à la demande
    (`scripting.executeScript`, c'est ainsi que Bitwarden pose son
    remplissage) tournaient sans `chrome.*` : ils plantaient au démarrage.
    Ils reçoivent désormais l'API de l'extension, comme ses content scripts ;
  - quand plusieurs scripts d'une extension sont dans la page, le premier
    qui n'avait rien à répondre à un `tabs.sendMessage` fermait l'échange
    avant que celui qui répond le fasse. Comme dans Chrome, « pas de
    réponse » ne vaut plus qu'une fois tous les scripts passés.

## [0.12.1] - 2026-11-16

### Corrigé

- La popup de Bitwarden restait noire : le navigateur répondait « oui » à
  toute question sur les permissions, Bitwarden croyait donc avoir la
  permission optionnelle `privacy` et appelait `chrome.privacy`, qui
  n'existe pas ici. Les extensions n'ont désormais que les permissions de
  leur manifest et celles accordées ensuite, jamais celles d'une API que le
  navigateur n'a pas.

## [0.12.0] - 2026-11-16

### Ajouté

- Un onglet glissé hors de la fenêtre (ou lâché bien sous la barre
  d'onglets, quand la fenêtre est agrandie) s'ouvre dans une nouvelle
  fenêtre, à l'endroit lâché. Pas dans une fenêtre privée, ni pour le
  dernier onglet. Cette fenêtre partage le profil (cookies, historique,
  favoris) mais ses onglets ne sont pas rouverts au lancement suivant.

### Modifié

- La barre d'onglets fait office de barre de titre, dans le dégradé de
  l'interface : plus de barre de titre système sous Windows et Linux, des
  boutons réduire / agrandir / fermer dessinés par le navigateur ; on
  déplace la fenêtre par l'espace vide de la barre (double-clic : agrandir)
  et on la redimensionne par ses bords. Sous macOS, les boutons du système
  restent, posés sur le dégradé.
- Historique et favoris sont fusionnés avec le fichier à l'enregistrement,
  pour que plusieurs fenêtres d'un même profil ne s'écrasent pas.

## [0.11.0] - 2026-11-16

### Ajouté

- Réorganisation des onglets : glisser-déposer dans la barre d'onglets sur
  ordinateur, appui long puis glisser sur l'écran des onglets Android.
- Suggestions dans la barre d'adresse : les recherches déjà faites
  (retrouvées dans l'historique, quel que soit le moteur courant ou un moteur
  connu) puis les pages de l'historique, classées par début de saisie,
  fréquence et récence. ↑/↓ et Entrée, ou un clic, sur ordinateur.
- Navigation privée. Sur ordinateur, une fenêtre privée (Ctrl+Shift+N ou le
  bouton masque à gauche de l'avatar) : rien n'est écrit dans le profil, ni
  historique ni onglets à rouvrir ; cookies, cache et stockage des sites
  vont dans un dossier temporaire supprimé à sa fermeture (ou au lancement
  suivant après un plantage) ; les favoris se lisent sans se modifier, les
  réglages changés le restent en mémoire, et extensions, scripts
  utilisateur, synchro et mises à jour automatiques sont coupés. Elle se
  reconnaît à son gris anthracite et à sa pastille « Private ». Sur Android,
  des onglets privés : une bascule *Normal / Private* sur l'écran des
  onglets, une WebView à part dont les cookies et données sont supprimés
  avec le dernier onglet privé, hors historique, session et synchro.

### Modifié

- Un nouvel onglet (Ctrl+T, `+`) s'ouvre en dernière position.
- Un lien ouvert dans un nouvel onglet (`target="_blank"`, clic molette,
  `window.open`) s'ouvre juste après sa page, en arrière-plan : on reste sur
  la page courante. Sur Android, un message le signale.

## [0.10.0] - 2026-11-16

### Ajouté

- Bloqueur de pubs intégré, sans extension à installer, sur ordinateur et
  Android : les listes EasyList et EasyPrivacy (moteur `adblock-rust` de
  Brave), téléchargées au premier lancement puis vérifiées une fois par
  jour, bloquent publicités et traqueurs ; les emplacements de pub des sites
  que les listes nomment sont masqués. Un bouclier à côté du cadenas compte
  ce qui est bloqué sur la page (sauf sous macOS et Linux) et coupe le
  blocage pour un site ou partout ; ces réglages se synchronisent.

### Corrigé

- La popup des extensions s'ouvrait trop étroite (Bitwarden) : sa largeur
  était mesurée avant que sa page ait ses styles. Elle est remesurée jusqu'à
  ce que la page soit chargée et stable.

## [0.9.4] - 2026-11-16

### Corrigé

- Les popups d'extension clignotaient encore (Bitwarden) : replacées à
  chaque image, elles reprenaient leur taille par défaut puis étaient
  remesurées. Leur taille n'est plus oubliée tant qu'elles restent ouvertes,
  et elles ne bougent que si leur place change.

## [0.9.3] - 2026-11-16

### Corrigé

- Extensions : comme dans Chrome, leurs pages joignent les sites couverts par
  leurs permissions sans contrôle CORS (le navigateur fait la requête pour
  elles). La connexion à Bitwarden échouait (« Failed to fetch », ou erreur
  de déchiffrement dans la popup).

## [0.9.2] - 2026-11-16

### Corrigé

- Les popups d'extension clignotaient (Bitwarden) : leur page était remise
  en page toutes les 400 ms pour mesurer sa largeur. La largeur est mesurée
  une fois à l'ouverture, ensuite seule la hauteur est suivie.

## [0.9.1] - 2026-11-16

### Corrigé

- Extensions : `runtime.connect` et `tabs.connect` ouvrent de vrais ports
  entre les pages de l'extension et ses scripts (messages dans les deux sens,
  `onDisconnect`). La popup de Bitwarden, qui attend sa page de fond sur ces
  ports, restait en chargement.
- Extensions : les messages des pages de l'extension (popup, page de fond)
  portent leur origine, comme dans Chrome ; les extensions qui la vérifient
  (Bitwarden) ignoraient ces messages.

## [0.9.0] - 2026-11-16

### Ajouté

- Extensions : messagerie native (`runtime.connectNative`,
  `runtime.sendNativeMessage`), comme dans Chrome. Une extension qui a la
  permission `nativeMessaging` parle au programme installé qui l'accepte
  (Bitwarden Desktop, KeePassXC…), déclaré là où Chrome, Edge ou Chromium le
  cherchent (registre sous Windows, dossiers `NativeMessagingHosts` sur macOS
  et Linux). Pas sur Android.

### Corrigé

- Les croix des onglets ne fermaient pas l'onglet.
- Le `+` de nouvel onglet et les croix se voient au survol.

## [0.8.0] - 2026-11-16

### Modifié

- Nouveau look « licorne », sur ordinateur (Windows, macOS, Linux) et
  Android : dégradé violet → cyan de l'icône derrière les onglets (Android :
  derrière la barre d'adresse et les en-têtes), barres nuit, barre d'adresse
  en pilule, accents cyan. Le panneau latéral, les bandeaux et les écrans
  secondaires suivent le thème clair ou sombre du système. Sur ordinateur,
  la police de l'interface est celle du système (Segoe UI sous Windows).
- Android : les onglets s'affichent en grille de cartes, avec un aperçu de
  chaque page et son icône ; les couleurs ne suivent plus le fond d'écran.

### Corrigé

- Extensions sous Windows : leurs pages n'avaient pas `crypto.subtle`
  (Bitwarden et d'autres ne démarraient pas : « Could not locate Subtle
  crypto »). Ce qu'une extension gardait dans le stockage de ses propres
  pages (IndexedDB, localStorage) repart de zéro ; son `chrome.storage` est
  conservé.

### Retiré

- Le panneau latéral des outils de développement (0.6.0) : il demandait au
  moteur un port de débogage local ouvert en permanence. Les outils de
  développement s'ouvrent de nouveau dans leur fenêtre, sans port.

## [0.7.0] - 2026-11-16

### Ajouté

- Extensions installées à la main : *Choose .crx…* (un paquet) et *Load
  unpacked…* (le dossier d'une extension décompressée, comme « Charger
  l'extension non empaquetée » de Chrome) dans l'onglet *Extensions* ; le
  chemin d'un dossier marche aussi dans le champ. Recharger le même dossier
  met l'extension à jour.

### Corrigé

- Extensions : celles dont le script de fond est découpé en morceaux
  (webpack) ne démarraient jamais (« Unexpected identifier 'found' »).
- Windows : ce qu'on tapait dans la barre d'adresse partait dans les champs
  de la page.
- Le bouton `+` de nouvel onglet suit le dernier onglet au lieu d'être au
  bout de la barre.

## [0.6.1] - 2026-11-16

### Corrigé

- Android : la version 0.6.0 n'a pas pu être construite (lint de la demande
  de permission des téléchargements) ; 0.6.1 publie ses nouveautés.

## [0.6.0] - 2026-11-16

### Ajouté

- Outils de développement dans un panneau à droite de la page (Windows) :
  F12, Ctrl+Shift+I et le bouton `</>` l'affichent ou le masquent, il suit
  l'onglet actif ; clic droit sur le bouton (ou bouton du panneau) pour les
  ouvrir dans une fenêtre. Le moteur ouvre pour cela un port de débogage
  local (127.0.0.1).
- Cadenas : un clic montre si la connexion est chiffrée, son protocole et
  son chiffrement, et la chaîne de certificats du site (émis pour, par,
  domaines couverts, validité, numéro de série, SHA-256). Windows et Android.
- Android : sur la page d'une extension du Chrome Web Store, un bandeau
  propose de l'installer, sans copier le lien.

### Corrigé

- Windows : les raccourcis (Ctrl+T, Ctrl+W, Ctrl+L, F5...) ne marchaient pas
  quand la page avait le focus clavier.
- Téléchargements : sur ordinateur, les fichiers arrivaient dans
  Téléchargements sans rien dire ; la barre de notification annonce leur
  début et leur fin (*Open*, *Show in folder*). Sur Android, ils ne
  marchaient pas du tout.
- Android : la barre d'adresse perdait le focus pendant la saisie.

## [0.5.1] - 2026-11-15

### Ajouté

- Android : mises à jour automatiques. L'app regarde `latest.json` une fois
  par jour, propose la nouvelle version dans un bandeau (installer, plus
  tard, ignorer), télécharge l'APK, vérifie son SHA-256 et lance
  l'installateur d'Android ; *Check for updates* dans le menu force la
  vérification. Android demande une fois d'autoriser l'installation d'apps
  depuis le navigateur.

## [0.5.0] - 2026-11-15

### Ajouté

- Profils : chacun a son historique, ses favoris, ses onglets, ses
  extensions, ses cookies, ses réglages et sa synchro, et s'ouvre dans sa
  propre fenêtre. Avatar à droite de la barre d'outils, onglet *Profiles* du
  panneau latéral (ajouter, ouvrir, renommer, couleur, supprimer) ; nom du
  profil dans le titre de la fenêtre ; `--profile <id>` en ligne de commande.
- Outils de développement : F12, Ctrl+Shift+I (Cmd+Option+I sur macOS) ou le
  bouton `</>` de la barre d'outils ouvrent l'inspecteur de l'onglet actif,
  aussi dans les versions publiées ; *Inspect* dans l'onglet *Extensions*
  inspecte la page de fond d'une extension.
- Android : option *Web debugging* du menu, pour inspecter les pages depuis
  `chrome://inspect` sur un ordinateur (débogage USB).

### Modifié

- Rust 1.89 minimum.

## [0.4.1] - 2026-11-15

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
