# Thème « licorne » du navigateur desktop

Date : 2026-11-16 · Statut : validé (brainstorming), à planifier

## But

Rendre l'interface desktop (egui) plus jolie et plus personnelle, avec le
même rendu sur Windows, macOS et Linux. Aujourd'hui elle utilise le style
egui par défaut et sa police (Ubuntu Light).

Hors périmètre : l'app Android (cycle à part, même palette en Compose), les
pages web elles-mêmes, les icônes (Phosphor, inchangées), la mise en page
(les mêmes barres et panneaux, aux mêmes endroits).

## Direction retenue

Mélange des maquettes « Nuit licorne » et « Dégradé licorne » (variante 2) :
dégradé de l'icône derrière les onglets, barres nuit en dessous ; le panneau
latéral suit le thème clair/sombre du système.

## Palette

| Rôle | Couleur |
|---|---|
| Dégradé (gauche → milieu → droite, 120°) | `#3a246e` → `#5b3fa6` → `#2d8fb3` |
| Nuit (onglet actif, barre d'outils, barre de favoris) | `#241a3d` |
| Nuit profond (fond de la barre d'adresse) | `#171126` |
| Contour de la barre d'adresse | `#3f3270` |
| Texte sur nuit | `#e9e4ff` ; secondaire `#b9b0d8` |
| Accent cyan (cadenas, étoile active, focus) | `#5adcff` |
| Panneau sombre : fond / survol / bordure | `#1c1430` / `#2a1f47` / `#2f2350` |
| Panneau clair : fond / survol / bordure | `#faf8ff` / `#efeaff` / `#e6e0f5` |
| Accent du mode clair | `#5b3fa6` |

## Éléments

- **Bande d'onglets** : fond en dégradé sur toute la largeur. Onglets
  inactifs : texte blanc à 90 %, sans fond, survol blanc translucide. Onglet
  actif : fond nuit `#241a3d`, coins arrondis en haut seulement, collé à la
  barre d'outils (pas d'espace entre les deux). Croix de fermeture à 55 %
  d'opacité. Le `+` blanc suit le dernier onglet.
- **Barre d'outils** : fond nuit, icônes lavande `#e9e4ff`, désactivées à
  40 %, survol en carré arrondi `#3a2c63`. Barre d'adresse en pilule : fond
  `#171126`, contour `#3f3270`, contour cyan quand elle a le focus. Cadenas et
  étoile active en cyan.
- **Barre de favoris** : fond nuit, texte `#b9b0d8`, fine bordure basse
  `#3a2c63`.
- Ces trois barres sont sombres quel que soit le thème du système.
- **Panneau latéral, bandeaux (mises à jour, infos du site), barre de
  notification** : suivent le thème du système.
  - Sombre : fond `#1c1430`, sélecteur de section en pilules `#2a1f47`,
    section active en dégradé, champs `#140e24`, boutons principaux cyan
    avec texte nuit.
  - Clair : fond `#faf8ff`, pilules `#efeaff` / active `#5b3fa6`, champs
    blancs bordés `#e0d9f3`, boutons principaux violets avec texte blanc.
  - Lignes de liste (historique, favoris, extensions) : coins arrondis,
    survol à la couleur de survol du thème.

## Police

La police d'interface du système, lue au démarrage :

- Windows : `%WINDIR%\Fonts\segoeui.ttf` (Segoe UI) ;
- macOS : `/System/Library/Fonts/SFNS.ttf` (San Francisco), sinon
  `/System/Library/Fonts/Helvetica.ttc` ;
- Linux : le fichier que donne `fc-match -f '%{file}' sans-serif`.

Si rien n'est lisible (fichier absent, police refusée par egui), on garde la
police actuelle d'egui. Les icônes Phosphor restent ajoutées en repli comme
aujourd'hui. Les tailles de texte ne changent pas.

## Architecture

- Nouveau module `src/ui/theme.rs` :
  - `Palette` : les couleurs ci-dessus, en constantes nommées ;
  - `install(ctx)` : polices, `Visuals` sombre et clair (egui choisit selon
    le thème du système, `ThemePreference::System`), arrondis et espacements ;
  - des aides de dessin : `gradient_rect(painter, rect)` (maillage à
    couleurs de sommets, egui n'ayant pas de dégradé), `chrome_frame()`
    (cadre des barres sombres), `pill(...)`.
- Nouveau module `src/ui/system_font.rs` : trouve et lit la police du
  système (voir plus haut) ; `None` si introuvable.
- `src/ui/mod.rs` appelle `theme::install` à la place de la configuration
  de polices actuelle.
- `tab_strip.rs`, `toolbar.rs`, `omnibox.rs`, `bookmarks_bar.rs` : dessinent
  avec la palette « chrome » (toujours sombre) ; les panneaux du haut
  prennent `chrome_frame()`.
- `library.rs`, `updates.rs`, `site_info.rs`, barre de notification de
  `app.rs` : utilisent les `Visuals` du thème courant (aucune couleur codée
  en dur), plus les pilules pour le sélecteur de section.
- Pas de nouvelle dépendance.

## Erreurs

- Police introuvable ou illisible : journal `warn`, police d'egui.
- `fc-match` absent (Linux) : même repli.

## Vérification

- Tests unitaires : contraste (WCAG, au moins 4,5:1) du texte principal sur
  le nuit, sur le panneau sombre et sur le panneau clair, et du texte des
  onglets inactifs (blanc à 90 %) sur la couleur la plus claire du dégradé
  (au moins 3:1 ; sinon on assombrit ce bout du dégradé jusqu'à l'atteindre) ; repli de `system_font` quand le fichier n'existe pas.
- Build et tests sur Windows ; la CI construit macOS et Linux.
- À l'œil : l'app lancée sur un profil isolé (`BU_PROFILE_DIR`), en mode
  sombre puis clair, capture validée par l'utilisateur.
