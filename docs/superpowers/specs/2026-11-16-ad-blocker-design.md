# Bloqueur de pubs intégré

Date : 2026-11-16 · Statut : validé (brainstorming), à planifier

## But

Bloquer de base les publicités et les traqueurs, sans extension à installer,
sur ordinateur (Windows, macOS, Linux) et Android, avec un bouclier qui
montre ce qui est bloqué et permet de couper le blocage pour un site.

Hors périmètre : bannières de cookies, fenêtres « abonnez-vous »,
anti-anti-adblock, listes au choix et filtres personnels ; masquage
cosmétique **générique** (règles valables sur tous les sites, à appliquer en
scannant les classes et ids des pages) — étape suivante.

## Moteur

- Crate `adblock` (adblock-rust, celui de Brave, MPL-2.0), version 0.13 ;
  fonctionnalité `content-blocking` pour la conversion WebKit.
- Listes : EasyList (`https://easylist.to/easylist/easylist.txt`) et
  EasyPrivacy (`https://easylist.to/easylist/easyprivacy.txt`).
  - Téléchargées au premier lancement, puis vérifiées une fois par jour
    (au démarrage et au retour sur l'app, comme les mises à jour), avec
    `If-Modified-Since` / `ETag` pour ne pas tout retélécharger.
  - Rangées dans `<dossier de données de l'app>/blocker/` (commun aux
    profils) : les listes brutes, et le moteur compilé sérialisé
    (`engine.dat`) pour démarrer sans re-analyser.
  - Pas de liste embarquée dans le programme (licences GPL/CC-BY-SA des
    listes) : au tout premier lancement, le blocage commence quand les
    listes sont arrivées.
  - Le moteur est construit hors du fil de l'interface et remplace
    l'ancien d'un coup.
- Décision pour une requête : bloquée si le bloqueur est actif, si le site
  de la page n'est pas dans la liste blanche, si ce n'est pas la page
  elle-même (`main_frame`, jamais bloquée) et si le moteur dit « bloquer »
  (en tenant compte des exceptions `@@` des listes).

## Où le blocage s'applique

- **Windows** (WebView2) : le gestionnaire `WebResourceRequested` existant
  (`webview2_blocking.rs`), installé sur chaque onglet même sans
  extension ; il consulte d'abord les règles des extensions, puis le
  bloqueur. Une requête bloquée reçoit la même réponse vide que pour les
  extensions.
- **Android** : `MobileBrowser::intercept`, appelé par
  `shouldInterceptRequest`, consulte aussi le bloqueur.
- **macOS, Linux** (WebKit) : le moteur convertit ses règles réseau au
  format des bloqueurs de contenu WebKit (`content-blocking`) ; la liste est
  compilée et ajoutée aux vues comme celles des extensions
  (`webkit_rules.rs`). La liste blanche y devient une règle
  `ignore-previous-rules` par site.
- **Masquage cosmétique (sites précis)** : un script de page, injecté dans
  chaque onglet, demande au navigateur les sélecteurs à masquer pour son
  adresse (opération du hub `blocker.cosmetic`), puis les injecte en
  `<style>` (`display: none !important`). Rien si le site est en liste
  blanche ou le bloqueur coupé.
- Les règles des extensions continuent de s'appliquer en plus.

## Réglages

- `Settings` (domaine) gagne `blocker: bool` (vrai par défaut) et
  `blocker_allowlist: Vec<String>` (noms d'hôtes ; un hôte couvre ses
  sous-domaines). Lus et écrits comme le reste des réglages, synchronisés
  avec eux. Absents d'un ancien fichier : valeurs par défaut.

## Bouclier

- **Desktop** : icône bouclier dans la barre d'outils, à droite du
  cadenas, avec un badge (nombre d'éléments bloqués sur la page, rien si
  zéro). Un clic ouvre un panneau egui sous la barre :
  - « N éléments bloqués sur cette page » (« actif » sans chiffre sur
    macOS et Linux, où WebKit ne dit pas ce qu'il bloque) ;
  - interrupteur « Bloquer sur ce site » (liste blanche) ;
  - interrupteur « Bloqueur de pubs » (général) ;
  - « Listes : EasyList, EasyPrivacy · mises à jour le … », ou « listes pas
    encore téléchargées ».
  Changer un interrupteur enregistre le réglage et recharge la page.
- **Android** : icône bouclier dans la barre d'adresse, à côté du cadenas ;
  même contenu dans une fenêtre de dialogue.
- **Compteur** : par onglet, remis à zéro au début de chaque nouvelle page
  (navigation principale), incrémenté à chaque requête bloquée par le
  bloqueur (pas par les extensions).

## Erreurs

- Téléchargement raté : on garde le moteur en cache ; journal `warn`.
- Liste corrompue (analyse impossible ou vide) : on garde l'ancienne.
- Rien en cache et pas de réseau : pas de blocage, le panneau le dit.
- Cache du moteur illisible (autre version d'adblock-rust) : on le
  reconstruit depuis les listes.

## Tests

- Moteur, sur une petite liste écrite dans le test : une pub est bloquée,
  une requête normale passe, une exception `@@` passe, la page principale
  n'est jamais bloquée, la liste blanche (sous-domaines compris) coupe le
  blocage, le bloqueur coupé ne bloque rien, les sélecteurs cosmétiques d'un
  site sont donnés et pas ceux d'un autre.
- Réglages : lecture d'un ancien fichier (valeurs par défaut), aller-retour,
  fusion à la synchro.
- Conversion WebKit : une règle réseau devient une action `block`, la liste
  blanche une `ignore-previous-rules`.
- À l'œil, par l'utilisateur : un site chargé de pubs (Le Monde), le
  compteur, la liste blanche qui fait revenir les pubs, Android.
