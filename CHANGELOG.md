# Changelog

Toutes les évolutions notables de browsingUnicorns. Format :
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), versions
[SemVer](https://semver.org/lang/fr/).

## [0.2.1] - 2026-11-15

### Corrigé

- L'application installée par le `.msi` ne démarrait pas : WebView2 voulait
  écrire ses données (cookies, cache) dans Program Files. Elles sont maintenant
  dans le profil, `<data>/browsing-unicorns/webview`.

## [0.2.0] - 2026-11-15

### Ajouté

- Application Android (Kotlin + Jetpack Compose) sur le même cœur Rust que la
  version de bureau : onglets, barre d'adresse, historique, favoris, ouverture
  des liens venant des autres apps. L'APK est publié avec les autres fichiers.
- Synchro chiffrée de bout en bout entre appareils (favoris, historique,
  onglets ouverts, réglages) : onglet *Sync* du panneau latéral, appairage par
  code ou QR code. Les onglets des autres appareils peuvent être rouverts.
- Relais de synchro sur Cloudflare Workers (`relay/sync`) : il ne stocke que
  des données chiffrées, effacées après 180 jours sans mise à jour.

## [0.1.0] - 2026-11-15

Première version.

### Ajouté

- Navigation par onglets : nouvel onglet (Ctrl+T), fermeture (Ctrl+W, clic
  molette), passage de l'un à l'autre (Ctrl+Tab). Les liens « nouvelle
  fenêtre » s'ouvrent dans un onglet.
- Barre d'adresse : adresse complète, nom de site ou recherche (DuckDuckGo par
  défaut), Ctrl+L ; précédent / suivant, recharger, accueil ; cadenas HTTPS.
- Favoris : étoile dans la barre d'adresse (Ctrl+D) et barre de favoris.
- Historique avec recherche (Ctrl+H), effaçable.
- Les onglets ouverts sont rouverts au démarrage.
- `browsingUnicorns <adresse>` ouvre l'adresse dans un nouvel onglet.
- Mises à jour automatiques : vérification quotidienne, bandeau pour installer,
  remettre à plus tard ou ignorer une version ; « Check for updates » dans
  l'onglet About du panneau latéral.
- Installateur Windows (.msi), version portable (.exe), AppImage Linux et image
  disque macOS (.dmg).
