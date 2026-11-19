# Scriptlets du bloqueur (pubs YouTube)

Date : 2026-11-19 · Statut : validé (brainstorming), à planifier

## But

Bloquer les pubs de YouTube, que le blocage réseau et le masquage
cosmétique actuels (EasyList, EasyPrivacy) ne retirent pas : il faut des
**scriptlets**, petits scripts injectés dans la page avant les siens (par
exemple retirer `adPlacements` de la réponse du lecteur). Sur Windows,
macOS, Linux et Android. Le mécanisme sert à tous les sites que les listes
couvrent, pas seulement YouTube.

Hors périmètre : Twitch (chantier suivant, qui réutilisera l'injection),
les menus et infobulles par-dessus la page (chantier à part), un YouTube
intégré dans un autre site (il n'a les scriptlets que si `youtube.com` est
déjà enregistré dans l'onglet), les filtres personnels.

## Listes et ressources

- Deux listes de plus dans `LISTS` (`crates/infrastructure/src/blocker.rs`),
  téléchargées, mises en cache et vérifiées une fois par jour comme
  EasyList :
  - `ublock-filters.txt` :
    `https://ublockorigin.github.io/uAssets/filters/filters.txt` ;
  - `ublock-quick-fixes.txt` :
    `https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt`.
  Syntaxe uBlock (`##+js(...)`…) : adblock-rust la lit ; ce qu'il ne gère
  pas est ignoré.
- Les ressources des scriptlets : `resources.json` de Brave
  (`https://raw.githubusercontent.com/brave/adblock-resources/master/dist/resources.json`),
  au format `Vec<adblock::resources::Resource>`, téléchargé et mis en cache
  au même rythme (`blocker/resources.json`), chargé par
  `Engine::use_resources`. Rien d'embarqué dans le programme (licences
  GPL), comme pour les listes.
- `engine.dat` ne contient pas les ressources : elles sont rechargées depuis
  le cache à chaque construction ou lecture du moteur. Sans fichier de
  ressources, les règles `+js` ne donnent rien ; le reste du bloqueur
  marche.
- Le bouclier nomme les nouvelles listes dans sa ligne « Listes : … ».

## Moteur

- `Blocker::scriptlets(url) -> String` : le script à injecter pour le site
  de `url`, `url_cosmetic_resources(url).injected_script` (exceptions
  `#@#+js` des listes comprises).
- Sur le `BlockerHandle` partagé (comme `cosmetic_css`) : chaîne vide si le
  bloqueur est coupé, si le site est en liste blanche ou s'il n'y a pas
  encore de moteur.

## Injection, au départ de la navigation

Fonction pure commune (`crates/infrastructure`) : pour une adresse et le
script du moteur, le script **gardé**
`if (location.hostname === "<hôte>") { try { … } catch (e) {} }` et l'hôte
qui sert de clé. Chaque vue garde l'ensemble des hôtes déjà enregistrés :
un hôte n'est enregistré qu'une fois, les scripts s'accumulent (un par site
visité, pour la vie de la vue).

Quand la **page principale** d'un onglet commence une navigation : calcul de
`scriptlets(url)` ; s'il n'est pas vide et que l'hôte n'est pas encore
enregistré, ajout du script gardé en début de document :

- **Windows** : dans le gestionnaire de navigation wry existant
  (`NavigationStarting`, `wry_engine.rs`), qui reçoit l'accès à la vue
  WebView2 juste après sa création ; `AddScriptToExecuteOnDocumentCreated`.
- **macOS / Linux** : le même gestionnaire wry (décision de navigation
  WebKit) ; `WKUserScript` / `UserScript` GTK en début de document, à côté
  des scripts de page existants (`page_injection.rs`), qui restent.
- **Android** : `shouldOverrideUrlLoading` (liens, redirections) et juste
  avant `loadUrl` (adresse tapée, onglet restauré) appellent
  `scriptlets(url)` côté Rust (via `MobileBrowser`, comme `intercept`), puis
  `WebViewCompat.addDocumentStartJavaScript(view, script,
  setOf("https://<hôte>"))`. Sans `DOCUMENT_START_SCRIPT` : au mieux, par le
  `evaluateJavascript` de `onPageStarted`.

Quand un réglage du bloqueur change (interrupteur général, liste blanche),
tous les scripts de scriptlets des vues sont retirés et l'ensemble des
hôtes vidé ; la page est déjà rechargée par le bouclier.

Fenêtres privées : pareil, tout en mémoire.

## Erreurs

- Liste ou ressources qui ne se téléchargent pas : on garde le cache ;
  journal `warn`. Ni cache ni réseau : pas de scriptlets, le reste bloque.
- Scriptlet qui lève une exception : arrêté par le `try/catch` de la garde,
  la page continue.
- Injection refusée (WebView2, WebKit, Android) : journal `warn`, la page se
  charge normalement.
- Site cassé par une règle : « Bloquer sur ce site » du bouclier la coupe.

## Tests

- `blocker.rs`, sur une petite liste et une ressource écrites dans le test :
  `site.test##+js(nom)` donne le script sur `site.test` et rien ailleurs ;
  `#@#+js(nom)` l'annule ; liste blanche, bloqueur coupé, pas de moteur :
  chaîne vide ; moteur relu depuis `engine.dat` + ressources rechargées :
  les scriptlets sont encore là ; ressources absentes : pas de scriptlet,
  le blocage réseau marche.
- Fonction de garde : le script ne vise que son hôte ; un hôte déjà
  enregistré ne l'est pas deux fois ; le vidage remet à zéro.
- Téléchargement : les nouvelles listes et `resources.json` passent par le
  même cache que les listes actuelles (téléchargement factice, comme les
  tests existants).
- Sur une build de dev Windows (par Claude) : une page locale et une règle
  `+js` maison, le scriptlet passe avant le script de la page dès le premier
  chargement ; puis YouTube, la réponse du lecteur sans pubs
  (`adPlacements` absent ou vide) via DevTools.
- À l'œil, par l'utilisateur : YouTube sur Windows et Android ; macOS et
  Linux par la CI (compilation, tests).
