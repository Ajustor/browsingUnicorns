# Onglets à gauche — conception

Date : 2026-11-26. Desktop uniquement (Android a son interface Kotlin).

## Réglage

- `DeviceSettings` (propre à l'appareil, comme `shrink_tabs`) :
  - `tab_position: TabPosition` (`Top` par défaut, `Left`) ;
  - `side_tabs_width: f32` (220 px par défaut, bornée 160–400) ;
  - `side_tabs_collapsed: bool`.
- `settings.toml` : `tabs = "top" | "left"`, `side_tabs_width`,
  `side_tabs_collapsed`. Un ancien fichier sans ces champs donne `top`.
- Panneau Réglages : « Position des onglets : En haut / À gauche ». Le choix
  du débordement (rétrécir / défiler) n'apparaît qu'en mode « En haut ».
- Menu contextuel d'un onglet : « Onglets à gauche » / « Onglets en haut ».

## Disposition

- Mode gauche : pas de barre d'onglets en haut. La barre d'outils devient la
  barre de titre : son espace vide fait glisser la fenêtre (double-clic :
  agrandir), les boutons ‑ □ × sont à sa droite (macOS : place des feux à
  gauche).
- Barre de favoris dessous, pleine largeur ; puis la colonne d'onglets
  (`SidePanel::left`) et la page. Le viewport de la page suit le
  `CentralPanel`.
- Plein écran : la colonne disparaît comme la barre d'onglets.

## Colonne (`src/ui/side_tabs.rs`)

- En-tête : pastille privée, nouvel onglet, recherche d'onglets, replier.
- Onglets épinglés : grille d'icônes en haut.
- Groupes : ligne d'en-tête (couleur, nom, chevron, menu du groupe), onglets
  indentés dessous ; un groupe replié cache ses onglets.
- Onglet : favicon, titre coupé à la largeur (tooltip egui), bouton son,
  × au survol ou sur l'actif. Clic : activer ; clic milieu : fermer ;
  menu contextuel identique à la barre du haut.
- Glisser-déposer vertical : déplacer ; sur un en-tête de groupe : le
  rejoindre ; hors de la fenêtre : nouvelle fenêtre.
- Bord droit tirable, largeur enregistrée. Repliée : favicons seuls (titre en
  tooltip), groupes en barre de couleur.
- Partagé avec `tab_strip.rs` plutôt que dupliqué : menus d'onglet et de
  groupe, boutons de fenêtre, détachement, calcul de la position de dépôt.

## Tests

- Unitaires : position de dépôt selon y, bornes de largeur, `settings.toml`
  (ancien fichier → `top`, aller-retour `left`).
- Lancement de l'app, vérification visuelle des deux modes.

## Hors périmètre

Android, onglets à droite.
