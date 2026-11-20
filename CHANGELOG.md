# Changelog

Toutes les évolutions notables de browsingUnicorns. Format :
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), versions
[SemVer](https://semver.org/lang/fr/).

## [0.17.2] - 2026-11-20

### Corrigé

- Une vidéo mise en plein écran depuis une fenêtre maximisée laissait une
  bande en haut ou sur les côtés de l'écran (par exemple une barre de
  titre sur Disney+, un liseré sur Crunchyroll) : la fenêtre quitte
  maintenant l'état maximisé le temps du plein écran et le retrouve
  après (Windows).

## [0.17.1] - 2026-11-20

### Modifié

- Android : les mises à jour de l'app s'installent comme le font les
  boutiques d'applications (session d'installation d'Android) au lieu
  d'ouvrir le fichier téléchargé. Après une première mise à jour faite
  ainsi, les suivantes peuvent s'installer sans confirmation (Android 12 et
  plus). Un échec d'installation est expliqué.

## [0.17.0] - 2026-11-20

### Ajouté

- L'interface existe en français et en anglais, avec les messages d'erreur.
  Sur ordinateur, *Réglages > Langue* : *Automatique* (la langue du
  système), *English* ou *Français* ; le choix vaut pour tout l'appareil
  et s'applique tout de suite. Sur Android, l'app suit la langue du
  système, ou celle choisie pour elle dans les réglages d'Android (13 et
  plus).

## [0.16.0] - 2026-11-20

### Ajouté

- Les publicités YouTube sont bloquées : le bloqueur reprend les listes
  *uBlock filters* et *Quick fixes* d'uBlock Origin, avec leurs scriptlets
  (de petits scripts lancés avant ceux de la page pour retirer les
  publicités). Cela vaut pour tous les sites que ces listes couvrent, sur
  ordinateur comme sur Android. *Bloquer sur ce site* les désactive comme le
  reste.
- Les publicités Twitch sont contournées, sur ordinateur comme sur Android,
  avec le script *vaft* de TwitchAdSolutions. Twitch glisse ses publicités
  dans le flux vidéo lui-même : le plus souvent, la coupure est remplacée
  par le flux en basse qualité (360p) plutôt que retirée. *Bloquer sur ce
  site* le désactive comme le reste.

### Corrigé

- Les infobulles et les menus de l'interface (barre d'outils, onglets,
  favoris, téléchargements, liste du moteur de recherche) étaient cachés
  par la page : ils passent maintenant par-dessus (Windows).
- Certaines extensions du Chrome Web Store ne s'installaient pas (par
  exemple heylogin, qui demande un navigateur récent) : la boutique
  répondait « aucune extension avec cet identifiant ».
- Les extensions de Microsoft Edge Add-ons s'installent désormais, avec leur
  lien ou leur identifiant.
- Le bouton « Ajouter à Chrome » de la boutique installe maintenant
  l'extension (son téléchargement échouait). La boutique affiche encore son
  message « téléchargement annulé », mais l'extension est bien installée.

## [0.15.1] - 2026-11-19

### Corrigé

- La barre des téléchargements : les cartes descendaient en escalier
  au lieu d'être alignées, et les boutons *Open folder*, *Clear* et la
  croix restaient en haut au lieu d'être centrés sur elles.

## [0.15.0] - 2026-11-18

### Ajouté

- Rouvrir le dernier onglet fermé : Ctrl+Maj+T, clic droit sur le `+`, ou
  *Reopen closed tab* dans le menu sur Android. Il revient à sa place ; les
  onglets privés ne reviennent pas.
- Un onglet *Settings* dans le panneau latéral (un écran *Settings* sur
  Android) :
  - page d'accueil et moteur de recherche (DuckDuckGo, Google, Bing,
    Brave, Ecosia, Qwant, Startpage) ;
  - téléchargements : le dossier où ils arrivent, ou demander où
    enregistrer chaque fichier ;
  - proposer d'enregistrer les mots de passe tapés dans les pages
    (Windows ; une extension comme Bitwarden marche dans tous les cas) ;
  - le zoom retenu pour chaque site, et le remettre à 100 % ;
  - effacer l'historique, les cookies et données des sites, le cache.
  Le dossier de téléchargement, les mots de passe et le zoom restent
  propres à chaque appareil ; la page d'accueil et le moteur de recherche
  se synchronisent.
- Zoom par site (Windows) : Ctrl+molette ou Ctrl +/- sur un site est
  retenu pour ses pages suivantes ; la barre d'outils affiche le zoom
  quand il n'est pas à 100 %, un clic le remet à 100 %.
- Les vidéos en plein écran (YouTube…) prennent tout l'écran, sans les
  barres du navigateur ; Échap ou le bouton de la vidéo en sort (Windows
  et Android).
- L'icône des sites dans les onglets (Windows).
- Un haut-parleur sur les onglets qui jouent du son ; un clic coupe le
  son de l'onglet, un autre le remet (Windows).
- Dossiers de favoris : chaque favori peut être rangé dans un dossier
  (bouton dossier dans la liste des favoris) ; la barre de favoris montre
  les dossiers, qui s'ouvrent dans le panneau latéral. Synchronisés (une
  version plus ancienne les ignore).
- Importer les favoris d'un autre navigateur (onglet *Settings*) : Chrome,
  Edge, Brave et Vivaldi sont lus directement, Firefox, Safari et les
  autres par le fichier HTML qu'ils exportent. Ils arrivent dans un
  dossier « Imported from … », avec leurs propres dossiers.

### Corrigé

- Certains sites restaient bloqués avec le bloqueur de pubs, par exemple
  une bannière de cookies impossible à fermer sur monnaiedeparis.fr : un
  script bloqué renvoie maintenant un script vide plutôt qu'une erreur,
  comme uBlock Origin, et les sites qui chargent leurs scripts en chaîne
  ne cassent plus.

## [0.14.5] - 2026-11-18

### Modifié

- Android : toucher la barre d'adresse sélectionne toute l'adresse, comme
  sur ordinateur.

## [0.14.4] - 2026-11-18

### Modifié

- Un clic dans la barre d'adresse (ou Tab jusqu'à elle) sélectionne toute
  l'adresse, prête à être remplacée ; un second clic place le curseur.

### Corrigé

- « Set as default » ouvrait le dossier Documents au lieu de la page des
  applications par défaut dans les Paramètres (Windows).

## [0.14.3] - 2026-11-18

### Corrigé

- La barre d'adresse ne suivait pas les sites qui changent de page sans
  recharger (YouTube d'une vidéo à l'autre, et beaucoup d'applications
  web) : elle garde maintenant l'adresse de la page affichée, et
  Précédent/Suivant suivent (Windows).
- Le bouton pour annuler un téléchargement (et celui pour l'afficher dans
  son dossier) ne réagissait pas : la carte du téléchargement prenait le
  clic à sa place.

## [0.14.2] - 2026-11-17

### Corrigé

- Les popups (connexion avec Google, paiement…) s'ouvraient dans un onglet,
  coupées de la page qui les avait ouvertes : la connexion n'aboutissait
  pas. Elles s'ouvrent maintenant dans leur propre fenêtre, et se ferment
  d'elles-mêmes une fois terminé (Windows). Les liens restent en onglet.

## [0.14.1] - 2026-11-17

### Ajouté

- Recharger sans le cache : Ctrl+Shift+R, Ctrl+F5, Maj+F5, ou Maj+clic sur
  le bouton Recharger.

### Corrigé

- Le bouton « Fill » de Bitwarden ne remplissait pas vraiment : les valeurs
  s'affichaient, mais les sites en React (et ce qui fonctionne pareil)
  ne les voyaient pas et envoyaient des champs vides. Une valeur posée par
  une extension passe désormais par le champ lui-même, comme dans Chrome.
- Les onglets rouverts au démarrage n'avaient pas Bitwarden (ni
  remplissage ni menu) tant qu'on ne les rechargeait pas : leur message
  partait avant que l'extension écoute. Il attend maintenant qu'elle soit
  prête.
- Les petits menus de Bitwarden sous les champs restaient vides : ils
  vérifient être servis depuis `chrome-extension:`, et sous Windows leur
  cadre ne pouvait pas parler au navigateur. Les deux sont réglés.

## [0.14.0] - 2026-11-17

### Ajouté

- browsingUnicorns propose de devenir le navigateur par défaut : une barre
  au démarrage (« Set as default », « Not now », « Don't ask again ») et
  une ligne dans « About ». Sous Windows, il s'inscrit comme navigateur
  pour l'utilisateur puis ouvre sa page dans les Paramètres, où Windows
  veut qu'on le choisisse ; sous Linux, il passe par `xdg-settings`. Pas
  sous macOS, où les liens arrivent par Apple Events, que la fenêtre ne
  reçoit pas encore.
- Un lien ouvert par une autre application arrive dans la fenêtre déjà
  ouverte du profil, dans un nouvel onglet, et la fenêtre passe devant ;
  avant, chaque lien lançait une nouvelle fenêtre complète.

## [0.13.0] - 2026-11-17

### Ajouté

- Une barre de téléchargements en bas de la fenêtre, à la place de la
  ligne d'état : une carte par fichier, avec son icône, où il en est
  (« 730 MB of 1.00 GB · 71% », barre de progression), et ce qu'on peut en
  faire : l'annuler pendant le transfert (Windows), l'ouvrir (clic sur la
  carte) ou le montrer dans son dossier une fois fini, le retélécharger
  s'il a échoué. « Open folder » ouvre le dossier des téléchargements,
  « Clear » retire ceux qui sont finis.
- Un bouton Téléchargements dans la barre d'outils, cyan pendant un
  transfert, rouvre ou ferme cette barre.

La progression et l'annulation n'existent que sous Windows : ailleurs, la
barre de progression est animée sans pourcentage.

## [0.12.7] - 2026-11-17

### Ajouté

- Quand des onglets sont cachés d'un côté de la barre, une ombre et une
  flèche (‹ ou ›) le signalent sur ce bord ; la molette les fait défiler.

## [0.12.6] - 2026-11-17

### Modifié

- Les boutons sans cadre (barre d'outils, extensions, onglets, favoris,
  panneaux) prennent un fond arrondi sous le pointeur, plus marqué pendant
  le clic, et le curseur devient une main : on voit enfin quel bouton est
  survolé.

## [0.12.5] - 2026-11-17

### Modifié

- La barre de défilement des onglets n'apparaît plus : elle gênait les
  clics sur les onglets et le déplacement de la fenêtre. La molette fait
  défiler les onglets (sans Maj), et l'onglet actif est ramené en vue.
- Un peu plus d'espace au-dessus des onglets, pour saisir la fenêtre et la
  déplacer.

## [0.12.4] - 2026-11-17

### Corrigé

- Quand les onglets dépassent la largeur de la barre, un nouvel onglet (ou
  celui activé par Ctrl+Tab ou par une extension) restait hors de vue : la
  barre défile désormais jusqu'à l'onglet actif dès qu'il change.

## [0.12.3] - 2026-11-17

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
