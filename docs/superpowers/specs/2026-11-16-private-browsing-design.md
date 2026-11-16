# Navigation privée

Date : 2026-11-16 · Statut : validé (brainstorming), à planifier

## But

Naviguer sans rien laisser derrière soi : ni historique, ni onglets
restaurés, ni cookies, ni stockage des sites, une fois la navigation privée
fermée. Sur ordinateur : une fenêtre privée. Sur Android : des onglets
privés à part.

## Ordinateur : la fenêtre privée

- **Ouverture** : Ctrl+Shift+N, ou un bouton « masque » (`egui_phosphor`
  `MASK_HAPPY` ou équivalent) à gauche de l'avatar du profil. Elle lance le
  même exécutable dans un nouveau processus : `--profile <id courant>
  --private`.
- **Une fenêtre privée** :
  - ne prend pas le verrou du profil (la fenêtre normale le garde) ;
  - range le stockage du moteur (WebView2 / WebKit : cookies, cache,
    stockage des sites) dans `<dossier temporaire>/browsing-unicorns-private-<pid>-<aléa>/`,
    supprimé à la fermeture de la fenêtre ; au lancement de toute fenêtre,
    les dossiers `browsing-unicorns-private-*` dont le processus n'existe
    plus sont supprimés (restes d'un plantage) ;
  - garde l'historique et les onglets en mémoire (dépôts en mémoire, jamais
    écrits) ; ne restaure pas la session du profil ; part de la page
    d'accueil (ou de l'adresse passée en ligne de commande) ;
  - lit les réglages du profil, mais leurs changements (bouclier par site,
    interrupteur général) restent en mémoire ;
  - montre les favoris du profil en lecture seule : ajouter ou retirer un
    favori affiche la notice « Not available in a private window. » ;
  - ne charge aucune extension ni script utilisateur, ne synchronise pas, ne
    vérifie pas les mises à jour ;
  - garde le bloqueur de pubs actif (listes communes, lues seulement : la
    fenêtre privée ne les télécharge pas) ;
  - télécharge les fichiers dans le dossier Téléchargements, comme
    d'habitude.
- **Habillage** : la bande d'onglets en dégradé anthracite
  (`#2b2b33` → `#3a3a46` → `#2b2b33`) au lieu du violet, barres en
  `#1f1f26`, une pastille « Private » avec l'icône masque à gauche des
  onglets, et « (private) » à la fin du titre de la fenêtre.
- Les vues du panneau latéral qui n'ont pas de sens en privé (Sync,
  Extensions, Profiles) affichent « Not available in a private window. ».

## Android : les onglets privés

- **Écran Onglets** : une bascule « Normal / Private » en tête de la grille
  montre l'une ou l'autre série ; le « + » ouvre un onglet de la série
  affichée. Le compteur de la barre du bas compte la série de l'onglet
  affiché.
- **Isolement** : les onglets privés utilisent un profil WebView à part
  (`androidx.webkit` `ProfileStore`, nom `bu-private`) : cookies et
  stockage séparés. Quand le dernier onglet privé se ferme (et à chaque
  démarrage de l'app), le profil est supprimé
  (`ProfileStore.deleteProfile`).
- **Cœur Rust** : `open_tab` reçoit `private: bool` ; les onglets privés
  ne vont ni dans l'historique ni dans la session sauvegardée, ne sont pas
  envoyés par la synchro, et sont fermés à l'arrêt de l'app.
- **Habillage** : quand l'onglet affiché est privé, la barre du haut est en
  dégradé anthracite et la barre du bas en `#1f1f26` ; icône masque dans la
  barre d'adresse.
- **WebView trop ancien** (`WebViewFeature.MULTI_PROFILE` absent) : la
  bascule « Private » n'apparaît pas (pas de mode privé qui partagerait les
  cookies).

## Hors périmètre

- Extensions autorisées en privé ; favoris modifiables depuis la fenêtre
  privée ; onglets privés dans une fenêtre normale sur ordinateur.

## Tests

- Ordinateur : en privé, l'historique et la session ne sont pas écrits (les
  fichiers du profil ne changent pas) ; le dossier temporaire est créé puis
  supprimé ; un reste d'une fenêtre privée morte est nettoyé ; les réglages
  changés ne sont pas enregistrés ; les favoris refusent les ajouts ; aucune
  extension n'est chargée ; `--private` est lu en ligne de commande.
- Android (cœur Rust) : un onglet privé n'entre ni dans l'historique ni dans
  la session ni dans l'enregistrement de synchro.
- À l'œil, par l'utilisateur : l'habillage, une connexion à un site en
  privé qui a disparu après fermeture, Android.
