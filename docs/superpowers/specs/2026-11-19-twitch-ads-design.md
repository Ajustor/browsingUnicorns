# Pubs Twitch

Date : 2026-11-19 · Statut : validé (brainstorming), à implémenter

## But

Retirer les pubs de Twitch, sur ordinateur et Android, avec le bloqueur
intégré. Twitch insère ses pubs dans le flux vidéo côté serveur : ni les
listes ni les scriptlets d'uBlock ne les retirent. La méthode qui marche le
mieux en 2026 est le script **vaft** (TwitchAdSolutions, MIT) : il
intercepte les listes de lecture du lecteur (dans son `Worker`) et bascule
sur un flux sans pub pendant la coupure. Le dépôt d'origine est archivé ;
la suite maintenue est celle de **ryanbr**
(`https://github.com/ryanbr/TwitchAdSolutions`). Depuis mai 2026, la coupure
est le plus souvent remplacée par le flux en basse qualité (360p) plutôt
que retirée : c'est ce qu'on promet, pas plus.

Hors périmètre : les proxys (TTV LOL…, vie privée), Twitch Turbo, un
réglage à part pour Twitch.

## Source

- `vaft/vaft.user.js` du dépôt de ryanbr
  (`https://raw.githubusercontent.com/ryanbr/TwitchAdSolutions/master/vaft/vaft.user.js`),
  téléchargé au même rythme que les listes et mis en cache dans
  `blocker/twitch-vaft.user.js` (dans `known_files()`, donc demandé dès la
  mise à jour de l'app). Rien d'embarqué dans le programme.
- Gardé seulement s'il ressemble au bon script : en-tête `==UserScript==`
  et `@name` contenant `vaft`. Sinon (page d'erreur, autre fichier) : on
  garde la copie en cache, journal `warn`. Son échec ne fait pas échouer la
  mise à jour des listes.

## Injection

- Le moteur garde le script (`Blocker`, chargé depuis le cache à chaque
  construction ou lecture du moteur, comme les ressources des scriptlets).
- `Blocker::scriptlets(url)` y ajoute vaft quand l'hôte est `twitch.tv` ou
  un sous-domaine (`www.twitch.tv`, `m.twitch.tv`, `player.twitch.tv`…),
  après les scriptlets d'uBlock. Le prélude `scriptletGlobals` ne concerne
  que ceux d'uBlock.
- Tout le reste vient du chantier YouTube, sans code de plus : injection en
  début de document sur Windows, macOS, Linux et Android, garde par nom
  d'hôte, *Bloquer sur ce site* et l'interrupteur général qui le coupent.
- vaft gère lui-même les cadres (il ne tourne que dans le cadre principal
  ou les cadres d'intégration de Twitch).

## Bouclier, journal

- La ligne des listes du bouclier ajoute « Twitch (vaft) » (ordinateur et
  Android).
- CHANGELOG (Non publié, Ajouté) : pubs Twitch, avec la limite (le plus
  souvent remplacées par le flux en basse qualité).

## Tests

- `blocker.rs` : vaft arrive pour `https://www.twitch.tv/x` (et
  `player.twitch.tv`), pas pour `https://nottwitch.tv/` ni
  `https://twitch.tv.evil.test/` ; liste blanche ou bloqueur coupé : rien ;
  sans fichier : rien ; un téléchargement qui n'est pas vaft n'est pas
  gardé ; le moteur relu depuis `engine.dat` a encore vaft.
- À l'œil : build de dev sur une chaîne en direct, vaft actif (`Worker`
  remplacé, son journal dans la console) ; les pubs elles-mêmes, par
  l'utilisateur, à l'usage.
