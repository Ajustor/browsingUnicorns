# Écran scindé — conception

Date : 2026-11-26. Desktop. Modèle : la « vue scindée » de Chrome.

## Comportement

- Deux onglets côte à côte dans la zone de la page, séparés par une barre
  qu'on tire (10 % à 90 %, 50 % par défaut, mémorisé par paire).
- Le panneau actif (celui de l'onglet actif) est entouré d'un fin liseré :
  barre d'adresse, raccourcis et extensions le suivent. Cliquer dans l'autre
  panneau le rend actif (Windows : `GotFocus` de la vue ; ailleurs, on passe
  par son onglet).
- Activer un onglet d'une paire affiche la paire ; un autre onglet remplace
  l'affichage.
- Menu d'un onglet :
  - un onglet inactif : « Afficher à côté de l'onglet actif » ;
  - l'onglet actif : « Scinder avec un nouvel onglet » ;
  - un onglet d'une paire : « Fermer l'écran scindé ».
- Dans la barre et la colonne, les deux onglets d'une paire se suivent et le
  partenaire de l'onglet actif est marqué comme affiché.

## Domaine (`Session`)

- `splits: Vec<Split>`, `Split { first: TabId, second: TabId, ratio: u16 }`
  (ratio en pour mille).
- Invariants, rétablis après chaque changement (`prune_splits`) : les deux
  onglets existent, se suivent (`second` juste après `first`), ont le même
  groupe et le même état épinglé ; un onglet est dans une paire au plus.
  Déplacer, épingler, grouper, fermer ou détacher un onglet d'une paire la
  défait donc d'elle-même.
- `split(a, b)` place `b` juste après `a` (dans le groupe de `a`) et crée la
  paire ; `unsplit(tab)` ; `set_split_ratio(tab, ratio)` ;
  `split_of(tab) -> Option<&Split>`.
- `SessionSnapshot` / `session.json` : `splits: [{first: <index>, ratio}]`.

## Moteur

- `WebEngine::show_panes(&[(TabId, Viewport)])` : les vues listées à leur
  place, les autres cachées. Par défaut (moteurs sans scission) : la
  première seule, sur tout `set_viewport`.
- `show(tab)` d'une vue déjà affichée ne change pas la disposition.
- Windows : découpe des tooltips par vue (trous traduits dans chaque
  panneau) ; `EngineEvent::PageFocused { tab }` sur `GotFocus`.

## Interface

- `app.rs` calcule les panneaux depuis le `CentralPanel` : barre de 6 px,
  liseré de 2 px autour du panneau actif, glisser la barre change le ratio.
- `Browser::handle(PageFocused)` active l'onglet s'il est le partenaire de
  l'onglet actif.

## Tests

- Domaine : création, ordre, invariants (déplacer, fermer, épingler,
  grouper), ratio borné, aller-retour du snapshot et de `session.json`.
- Application : `PageFocused` active le partenaire, pas un onglet quelconque.
- Lancement de l'app : deux pages côte à côte, barre tirée.

## Hors périmètre

Plus de deux panneaux, scission verticale, Android, « ouvrir le lien dans
l'écran scindé ».
