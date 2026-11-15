# browsing-unicorns-sync

Relais de synchronisation de browsingUnicorns (Cloudflare Worker + KV).

Il garde un blob chiffré par appareil d'un groupe de synchro et les rend aux
appareils du groupe. Le chiffrement (XChaCha20-Poly1305) se fait sur les
appareils avec une clé que le relais ne reçoit jamais : il ne peut lire ni
favoris, ni historique, ni onglets.

| Requête | Effet |
|---|---|
| `PUT /v1/groups/:group/devices/:device` | remplace le blob de l'appareil (1 Mio max) |
| `GET /v1/groups/:group` | `{"devices":[{"id","data","updated"}]}`, `data` en base64 |
| `DELETE /v1/groups/:group/devices/:device` | l'appareil quitte le groupe |

Chaque requête porte `Authorization: Bearer <jeton>`. Le jeton est dérivé de la
clé du groupe ; la première écriture enregistre son SHA-256 (le jeton lui-même
n'est jamais stocké), les requêtes suivantes doivent correspondre. Au plus 10
appareils par groupe ; un groupe inutilisé pendant 180 jours est oublié.

## Déployer

```sh
npm install
npx wrangler login
npx wrangler kv namespace create SYNC   # mettre l'id affiché dans wrangler.toml
npm run deploy
```

Puis mettre l'URL du Worker (`https://browsing-unicorns-sync.<compte>.workers.dev`)
dans `DEFAULT_RELAY` (`src/ui/sync.rs`) et dans l'app Android.

## Tests

```sh
npm test
```
