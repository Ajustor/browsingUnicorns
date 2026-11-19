# Infobulles et menus par-dessus la page

Date : 2026-11-19 · Statut : validé (brainstorming), à implémenter

## Problème

La page de l'onglet est une fenêtre native (WebView2 sur Windows) posée
par-dessus la surface où egui dessine. Tout ce qu'egui dessine dans la zone
de la page est donc caché : les infobulles de la barre d'outils, des onglets
et des favoris (qui tombent sur la page), celles de la barre des
téléchargements (qui montent sur la page), la liste déroulante du moteur de
recherche dans les réglages.

## Principe

À chaque image, la fenêtre de la page reçoit une **région** : toute la page
moins les rectangles des calques egui de premier plan qui la chevauchent.
egui se voit à travers ces trous, la page reste vivante partout ailleurs
(vidéo comprise), et les clics dans un trou vont à egui (les menus restent
cliquables). Essai fait à la main sur la fenêtre `WRY_WEBVIEW` (conteneur de
wry, parent des fenêtres de Chromium) : la surface d'egui apparaît bien dans
le trou.

## Calcul des trous (partagé, testé)

- Après le dessin de l'interface, les calques egui affichés d'ordre
  `Foreground` et `Tooltip` (infobulles, listes déroulantes, menus,
  fenêtres flottantes ; pas les panneaux, déjà à côté de la page) donnent
  leurs rectangles (`Memory::areas`, rectangle de chaque calque).
- Fonction pure `page_holes(layers, page_rect, pixels_per_point) ->
  Vec<Viewport>` : garde l'intersection de chaque rectangle avec la page,
  en pixels physiques relatifs au coin de la page, arrondis vers
  l'extérieur ; ignore les rectangles vides ou hors de la page.

## Moteur

- Port `WebEngine` : `set_page_holes(&mut self, holes: &[Viewport])`, sans
  effet par défaut.
- `WryEngine`, Windows : sur la fenêtre `WRY_WEBVIEW` de l'onglet affiché
  (le parent de la fenêtre que donne le contrôleur WebView2, ou ce que wry
  expose), `SetWindowRgn` avec la région « page moins les trous » ; sans
  trou, région retirée (`SetWindowRgn(hwnd, NULL)`). Rien n'est fait si les
  trous n'ont pas changé depuis la dernière fois ; un changement d'onglet
  affiché ou de taille de la page recalcule.
- macOS, Linux : rien pour l'instant (masque de calque, forme GTK : plus
  tard). Android : pas concerné (menus natifs).
- La fenêtre de popup des extensions est une autre vue native, déjà
  au-dessus : pas concernée.

## Tests

- `page_holes` : un rectangle à cheval sur le bord de la page est coupé ;
  hors de la page : rien ; conversion en pixels avec arrondi vers
  l'extérieur (150 %) ; coordonnées relatives à la page.
- À l'œil (build de dev) : une infobulle de la barre d'outils entière
  par-dessus une page ; la page vivante autour ; la région retirée quand
  l'infobulle disparaît ; un menu déroulant cliquable par-dessus la page.
