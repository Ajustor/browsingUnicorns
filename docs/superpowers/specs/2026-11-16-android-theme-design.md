# Thème « licorne » de l'app Android

Date : 2026-11-16 · Statut : validé (brainstorming), à planifier

## But

Donner à l'app Android le style choisi pour le desktop
(`2026-11-16-desktop-theme-design.md`) et un écran Onglets en grille avec
aperçus. La disposition actuelle ne bouge pas : barre d'adresse en haut,
barre d'icônes en bas.

Hors périmètre : la disposition des barres, le contenu des écrans
(Historique, Favoris, Extensions, Synchro : seulement leurs couleurs et leur
en-tête), la police (Roboto, celle d'Android).

## Palette

Celle du desktop. Plus de couleurs Material You tirées du fond d'écran.

| Rôle | Couleur |
|---|---|
| Dégradé (120°) | `#3a246e` → `#5b3fa6` → `#2d8fb3` |
| Nuit (barre du bas) | `#241a3d` |
| Nuit profond (barre d'adresse) / contour | `#171126` / `#3f3270` |
| Texte et icônes sur nuit | `#e9e4ff` ; désactivées à 40 % |
| Accent cyan | `#5adcff` |
| Thème sombre : fond / surfaces (cartes) / survol | `#1c1430` / `#241a3d` / `#2a1f47` |
| Thème clair : fond / surfaces / accent | `#faf8ff` / `#efeaff` / `#5b3fa6` |
| Texte : sombre / clair | `#ece7ff` / `#241b3d` |

## Écran du navigateur

- **Haut** : un bloc en dégradé qui passe sous la barre d'état (l'app est
  déjà en plein écran, `enableEdgeToEdge`), contenant la barre d'adresse :
  pilule `#171126` bordée `#3f3270`, texte `#e9e4ff`, cadenas cyan, icône
  « info » lavande, bouton recharger lavande.
- **Progression** : trait de 3 dp en dégradé violet → cyan sous la barre
  d'adresse pendant le chargement.
- **Bas** : barre nuit `#241a3d` qui descend sous la barre de navigation
  du système, icônes `#e9e4ff`, étoile active cyan ; le compteur d'onglets
  garde son carré.
- Ces deux barres sont toujours sombres.
- Bandeaux (Web Store, mise à jour), menu, fenêtres de dialogue : thème du
  téléphone.

## Autres écrans

En-tête en dégradé (sous la barre d'état), flèche retour, titre et actions
en blanc. Contenu sur le fond du thème du téléphone, lignes de liste en
cartes arrondies (12 dp) sur la couleur de surface.

## Écran Onglets

- Grille de 2 colonnes, cartes arrondies (14 dp) couleur surface :
  en haut favicon (16 dp), titre (une ligne), ✕ ; en dessous l'aperçu de la
  page (rapport 3:4, coupé en haut de page).
- L'onglet actif : contour cyan 2 dp.
- Sans aperçu : favicon en grand (ou pastille avec l'initiale du domaine)
  centré sur fond nuit, domaine dessous.
- `+` dans l'en-tête ; toucher une carte active l'onglet et revient à la
  page.

## Aperçus et favicons

- Aperçu : image de la page réduite au tiers, capturée quand l'onglet
  visible change (avant de le cacher) et à l'ouverture de l'écran Onglets
  (onglet actif). En mémoire seulement ; retirée quand l'onglet se ferme.
- Favicon : celle que la page annonce (`WebChromeClient.onReceivedIcon`), en
  mémoire, retirée avec l'onglet.
- Échec de capture (vue de taille nulle, mémoire) : pas d'aperçu, rien
  d'autre.

## Architecture

- `ui/Theme.kt` : la palette (`Unicorn`), les `ColorScheme` sombre et clair
  fixes, `UnicornGradient` (un `Brush`), et une fonction pure de contraste
  pour les tests.
- `ui/App.kt` : `Scaffold` sans marges d'encarts ; le bloc du haut gère la
  barre d'état, la barre du bas la barre de navigation.
- `ui/Lists.kt` : `Header` en dégradé, `PageRow` en carte.
- `ui/Tabs.kt` (nouveau) : l'écran Onglets en grille (sort de `Lists.kt`).
- `WebViews.kt` : `thumbnails` et `favicons` (états Compose), capture.

## Vérification

- Tests unitaires JVM (nouveaux : JUnit) : contrastes de la palette, mêmes
  seuils que sur desktop.
- `:app:compileDebugKotlin` et `:app:lintVitalRelease`.
- À l'œil sur le téléphone par l'utilisateur.
