# Scriptlets du bloqueur (pubs YouTube)

Date : 2026-11-19 · Statut : implémenté

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
  pas est ignoré. Ces deux listes ont droit aux scriptlets « trusted »
  (`trusted-*`, utilisés pour YouTube), comme dans uBlock Origin ; EasyList
  et EasyPrivacy non.
- Un fichier inconnu de la dernière vérification (`lists` de `state.json`
  comparé à `known_files()`, donc une liste ajoutée par une mise à jour de
  l'app) rend les listes « à vérifier » tout de suite, sans attendre le
  délai d'un jour. Un fichier absent parce que son téléchargement a échoué
  ne les rend pas « à vérifier » avant la fin de la journée.
- Les ressources des scriptlets : ceux d'**uBlock Origin**. Le
  `resources.json` public de Brave n'a que les ressources propres à Brave
  (aucun `set-constant`, `json-prune`… que les règles YouTube appellent),
  et uBlock publie ses scriptlets en modules JavaScript qu'adblock-rust ne
  lit plus : il faut du JavaScript pour les convertir.
  - `scripts/ublock-resources.mjs` (Node) importe
    `src/js/resources/scriptlets.js` d'uBlock et écrit chaque scriptlet au
    format `Vec<adblock::resources::Resource>` (fonction en
    `application/javascript`, aides `*.fn` en `fn/javascript`,
    dépendances, permission pour les `requiresTrust`).
  - `.github/workflows/ublock-resources.yml` le lance chaque jour (et à
    chaque changement du script) et publie `ublock-resources.json` comme
    pièce de la pré-version `ublock-resources` du dépôt (le site et le
    programme de mise à jour ne regardent que les versions stables).
  - L'app le télécharge
    (`https://github.com/Ajustor/browsingUnicorns/releases/download/ublock-resources/ublock-resources.json`)
    et le met en cache au même rythme que les listes
    (`blocker/ublock-resources.json`), chargé par `Engine::use_resources`.
    Rien d'embarqué dans le programme (licence GPL d'uBlock), comme pour
    les listes.
  - Vérifié : avec les listes uBlock, le script d'une page YouTube fait
    ~80 Ko (`set-constant`, `json-prune`, `trusted-replace`…) ; dans l'app,
    `playerResponse` est piégé et `adPlacements` a disparu.
- `engine.dat` ne contient pas les ressources : elles sont rechargées depuis
  le cache à chaque construction ou lecture du moteur. Sans fichier de
  ressources, les règles `+js` ne donnent rien ; le reste du bloqueur
  marche.
- Le bouclier nomme les nouvelles listes dans sa ligne « Listes : … ».

## Moteur

- `Blocker::scriptlets(url) -> String` : le script à injecter pour le site
  de `url`, `url_cosmetic_resources(url).injected_script` (exceptions
  `#@#+js` des listes comprises), précédé de `const scriptletGlobals = {};`
  quand il n'est pas vide (`safe-self.fn`, `prevent-xhr.fn` d'uBlock le
  lisent).
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

Quand la navigation d'un onglet commence (page principale sur Windows et
Android ; WebKit voit aussi celles des cadres, accepté : la garde sur
l'hôte suffit) : calcul de `scriptlets(url)` ; s'il n'est pas vide et que
l'hôte n'est pas encore enregistré, ajout du script gardé en début de
document :

- **Windows** : `webview2_scriptlets.rs::attach`, appelé par `wry_engine`
  après la création de la vue, branche son propre gestionnaire
  `NavigationStarting` ; `AddScriptToExecuteOnDocumentCreated`. Le
  changement du bloqueur est détecté dans le gestionnaire ; une époque
  (compteur de vidages) fait retirer les identifiants qui arrivent après un
  vidage. Si l'ajout est refusé, l'hôte est oublié : la navigation suivante
  réessaie. L'ajout est asynchrone et WebView2 ne l'attend pas (mesuré sur
  Twitch : document créé ~70 ms après `NavigationStarting`, ajout terminé
  ~450 ms après). Si la page de la navigation qui attend le script de son
  site est engagée (`ContentLoading`, même identifiant de navigation)
  avant la fin de l'ajout, elle est rechargée (`Reload`) une fois le script
  là ; si une autre navigation vers ce site est venue entre-temps, c'est
  elle qui est suivie ; si l'onglet est parti ailleurs, rien. Le
  rechargement garde l'historique tel que la page l'a fait (le
  `location.replace` d'une redirection, la nouvelle entrée d'un lien), le
  référent et l'initiateur. Il coûte un second chargement de la page (la
  première, sans scriptlets, a pu commencer à charger ses scripts) et une
  seconde visite dans l'historique du navigateur. Écartés après essai :
  annuler la navigation puis la refaire (ce qui l'a lancée ne se distingue
  pas : `IsUserInitiated` vaut vrai pour un `location.replace` sur minuterie,
  et `navigator.userActivation.hasBeenActive` est vrai sur une page que
  personne n'a touchée ; refaite, une redirection laissait sa page dans
  l'historique, et Retour y renvoyait) ; différer la requête du document
  (`WebResourceRequested`, `GetDeferral`) : l'ajout ne se termine plus et
  l'onglet reste vide. Pas rechargée : une navigation qui porte un corps
  (`Content-Type`, vu sur un POST de formulaire vers YouTube et absent des
  GET : le recharger renverrait le formulaire), ni celle de l'historique ou
  de nature inconnue (anciens runtimes) : le script vaut alors dès le
  document suivant. Une navigation partie avant que le moteur n'existe (au
  démarrage, le cache se charge ~100 ms après la restauration des onglets)
  est refaite une fois quand il arrive (`Scriptlets::catch_up`, appelé par
  `WryEngine::pump`), si son site a des scriptlets.
- **macOS / Linux** : `webkit_scriptlets.rs`, `Sites::navigating` appelé
  depuis le gestionnaire de navigation de wry ; `WKUserScript` / `UserScript`
  GTK en début de document, à côté des scripts de page existants
  (`page_injection.rs`), qui restent. Sous macOS, le retrait reconstruit les
  scripts du contrôleur sans ceux qui portent `site_scripts::MARKER`.
- **Android** : préparé dans `openView`, `navigate`,
  `shouldOverrideUrlLoading` (cadre principal), rechargement, et
  précédent/suivant (page de la liste d'historique où l'on va) ; appelle
  `scriptlets(url)` côté Rust (via `MobileBrowser`, comme `intercept`), puis
  `WebViewCompat.addDocumentStartJavaScript(view, script, setOf(origine))`,
  l'origine étant `schéma://hôte[:port]`. Suivi par onglet et par origine.
  Sans `DOCUMENT_START_SCRIPT` : au mieux, par le `evaluateJavascript` de
  `onPageStarted`.

Quand le bloqueur change (interrupteur général, liste blanche, nouveau
moteur), les scripts de scriptlets sont retirés et les hôtes oubliés :
sur ordinateur, paresseusement, vue par vue, à sa prochaine navigation ;
sur Android, tout de suite aux bascules du bouclier et quand
`updateBlockerLists()` rend vrai (nouveau moteur). La page est déjà
rechargée par le bouclier.

Fenêtres privées : pareil, tout en mémoire.

## Erreurs

- Liste ou ressources qui ne se téléchargent pas : on garde le cache ;
  journal `warn`. Ni cache ni réseau : pas de scriptlets, le reste bloque.
- Scriptlet qui lève une exception : arrêté par le `try/catch` de la garde,
  la page continue.
- Injection refusée : journal `warn` sous Windows et Android, la page se
  charge normalement ; l'ajout de WebKit est synchrone et ne peut pas
  échouer.
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
- Téléchargement : les nouvelles listes et `ublock-resources.json` passent par le
  même cache que les listes actuelles (téléchargement factice, comme les
  tests existants) ; l'ancien `resources.json` est supprimé à la mise à jour.
- Sur une build de dev Windows (par Claude) : une page locale et une règle
  `+js` maison, le scriptlet passe avant le script de la page dès le premier
  chargement ; puis YouTube, la réponse du lecteur sans pubs
  (`adPlacements` absent ou vide) via DevTools.
- À l'œil, par l'utilisateur : YouTube sur Windows et Android ; macOS et
  Linux par la CI (compilation, tests).
