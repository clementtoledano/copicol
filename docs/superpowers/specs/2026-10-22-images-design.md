# Images dans le presse-papiers — conception

Date : 22/10/2026 · Statut : validée

## Pourquoi

Un utilisateur a voulu retrouver une capture d'écran dans copicol et n'a rien trouvé : le watcher ne lit que du texte (`clipboard.get_text()` dans `watcher.rs`). Pour un utilisateur, une capture d'écran *est* un copier-coller : qu'elle n'apparaisse pas donne l'impression que l'outil est cassé.

L'utilisateur veut quatre choses à la fois :

1. **retrouver** une image copiée dans l'historique ;
2. la **recoller** plus tard dans une autre application ;
3. la **garder en favori** ;
4. l'**annoter** avec les outils d'une barre façon « Outil Capture » : sélection, crayon, formes (ligne, flèche, rectangle, ellipse, étape numérotée) et texte.

La maquette validée est dans `.superpowers/brainstorm/1275-1792664179/content/images-v1.html`.

## Objectifs

### Capture

Une image arrive dans le presse-papiers par l'un de ces chemins :

- Win+Maj+S ou Impr. écran ;
- « Copier l'image » dans un navigateur ;
- une copie depuis Paint ;
- **clic droit → Copier sur un fichier image dans l'Explorateur** (demandé explicitement).

Dans tous les cas, l'image entre dans l'historique comme n'importe quel élément.

### Affichage dans la liste

- Une miniature remplace la ligne de texte.
- Elle est accompagnée du libellé « Image », des dimensions (`1920 × 1080`) et du poids stocké (`412 Ko`).
- Les images ont leur propre type, `image`, qui fait apparaître un onglet **« Images »** dans la rangée d'onglets automatiques, comme les autres types.

### Recoller

- Entrée ou un clic remet l'image dans le presse-papiers **en tant qu'image (bitmap)** et cache la fenêtre, comme `copy_item` pour du texte.
- Le collage dans Teams, Word, Outlook ou Paint doit marcher.
- copicol ne simule toujours aucune frappe.

### Favoris

On peut épingler, nommer, ranger dans un dossier et désépingler une image, exactement comme un texte. Dans le menu du tray, un favori image apparaît sous son nom, et le cliquer copie l'image.

### Annotation

**Ctrl+E**, le bouton « Annoter » au survol de la ligne, ou « Modifier » dans un favori déplié ouvre l'éditeur d'annotation **dans la fenêtre copicol**. La touche E seule et le double-clic, envisagés au départ, ne sont pas possibles : la recherche a le focus, et un simple clic copie déjà. L'éditeur comprend :

- **En-tête** : titre « Annoter l'image », annuler/rétablir (↶ ↷, aussi Ctrl+Z / Ctrl+Y), bouton « Annuler » qui ferme sans rien changer, bouton « Enregistrer ».
- **Barre d'outils flottante en bas** :
  - sélection, pour déplacer et supprimer une annotation déjà posée tant que l'éditeur est ouvert ;
  - crayon (trait libre) ;
  - bouton de formes avec menu : ligne, flèche, rectangle, ellipse, étape ;
  - texte ;
  - palette de 5 couleurs (rouge par défaut, jaune, vert, bleu de l'accent, noir) ;
  - fermer.
- **Étape** : une pastille ronde numérotée. Les numéros s'incrémentent à chaque pose (1, 2, 3…) dans l'image en cours.
- **Enregistrer** aplatit les annotations dans l'image et **remplace l'image de l'élément** : l'original n'est pas conservé. Nom, dossier et statut de favori restent identiques. Dimensions, poids et miniature sont recalculés. Ensuite :
  - l'image annotée est **mise dans le presse-papiers**, via le watcher, qui met à jour son hash pour ne pas la recapturer ;
  - la **fenêtre se cache**, comme pour un collage ;
  - l'élément **remonte en tête** de l'historique.

  L'utilisateur peut coller tout de suite dans Teams ou Word. Comme le presse-papiers ne garde pas l'original, celui-ci ne revient pas dans l'historique au redémarrage de copicol.

Le comportement de « Enregistrer » a été proposé et accepté (« super »).

### Export et import des favoris

Les favoris image sont inclus dans l'export JSON et réimportables. Les fichiers d'export existants restent importables.

## Non-objectifs

Pas dans cette version :

- recadrer, redimensionner, flouter ou pixeliser ;
- reconnaissance de texte (OCR) ;
- recherche dans le contenu des images ;
- images animées (on ne garde que la première image d'un GIF) ;
- recoller une image sous forme de fichier ;
- copier plusieurs fichiers d'un coup.

## Ce qui ferait de cette version un échec, même si elle marche

- **L'historique se remplit d'images parasites.** Excel, Word et d'autres logiciels mettent à la fois du texte et une image dans le presse-papiers quand on copie des cellules ou un paragraphe. Si chaque copie Excel crée aussi une entrée image, c'est raté.
  - **Règle :** si le presse-papiers contient du texte non vide, on enregistre le texte, comme aujourd'hui, et on ignore l'image. On ne capture une image que s'il n'y a pas de texte.
- **copicol devient lent à ouvrir.** Ouvrir la fenêtre (Ctrl+Maj+V) doit rester instantané avec 100 éléments, dont beaucoup d'images. La liste ne charge que des miniatures, jamais les images complètes.
- **La base grossit sans limite.** Les images stockées sont compressées. Les éléments image non épinglés comptent dans la purge à 100 éléments.
- **Une image privée est capturée.** Le marqueur `ExcludeClipboardContentFromMonitorProcessing` est respecté pour les images comme pour le texte.
- **Une image recollée par copicol revient en doublon en tête d'historique.** Comme pour le texte, une écriture faite par copicol ne doit pas être recapturée au poll suivant.

## Contraintes

### Règles d'architecture du projet (`CLAUDE.md`)

- Le thread watcher reste l'**unique propriétaire du presse-papiers**. Lire et écrire des images passe par lui : nouvelle variante de `WatcherMsg` avec ack, sur le modèle de `Write`.
- Chaque nouvelle commande est câblée à 3 endroits : `commands.rs`, `invoke_handler` dans `lib.rs`, `api.ts`.
- L'interface `Item` d'`api.ts` reste le miroir de `db::Item`.
- `refresh_tray_menu` est appelé après tout changement de favoris.

### Autres contraintes

- Le frontend reste en TypeScript vanilla, sans framework ni bibliothèque de dessin. L'éditeur est un `<canvas>`.
- Tout est en français : interface, commentaires, commit.
- Tout reste local : aucun envoi réseau.
- La limite actuelle de 2 Mo (`MAX_CONTENT_LEN`) vise le texte. Une capture plein écran pèse environ 8 Mo en RGBA brut : il faut une limite propre aux images.
  - Mon choix : ignorer les images de plus de 50 mégapixels ou dont le PNG dépasse 20 Mo, avec un log comme pour le texte trop volumineux.
- Le dédoublonnage par hash SHA-256 s'applique aussi aux images, calculé sur les pixels décodés. La même capture recopiée remonte en tête au lieu d'être dupliquée.

## Choix techniques que je fais (à confirmer en lisant)

- **Stockage** : l'image est stockée en PNG dans SQLite (BLOB), avec une miniature PNG d'environ 200 px de large générée à l'insertion, plus largeur et hauteur.
  - La forme exacte est laissée au constructeur : table `images` liée à `items` ou colonnes ajoutées.
  - Contraintes : migration non destructive des bases existantes, `content` et la recherche texte ne doivent pas matcher des données binaires, l'unicité du hash reste valable.
- **Décodage et encodage** : la crate `image` (déjà présente dans `Cargo.lock` en dépendance transitive) gère PNG, JPEG, GIF, BMP et WebP pour les fichiers venant de l'Explorateur.
- **Fichiers copiés dans l'Explorateur** : capturés seulement si la copie contient **exactement un fichier** avec une extension image reconnue (`png`, `jpg`, `jpeg`, `gif`, `bmp`, `webp`). Le fichier est lu et décodé, puis enregistré comme une image ordinaire. Le recoller donne une image, pas le fichier.
- **Type `image`** : il ne fait pas partie de la reclassification manuelle (`set_kind`). Une image ne peut pas devenir « texte » ni l'inverse.
- **Modale d'édition de texte** : elle ne s'ouvre pas pour une image. Ctrl+E ou « Modifier » ouvre l'éditeur d'annotation à la place.
- **Export JSON** : le PNG est encodé en base64 dans un nouveau champ du `FavoriteEntry` et `version` passe à 2. Un fichier de version 1 s'importe comme avant.
- **Annotations pendant l'édition** : elles restent des objets éditables (sélection, déplacement, suppression avec Suppr) tant que l'éditeur est ouvert. Elles sont aplaties seulement à l'enregistrement.
- **Budget disque des images** : une capture d'écran pèse typiquement 200 à 500 Ko en PNG, mais le plafond par image va jusqu'à 20 Mo. En plus de la purge à 100 éléments, les images **non épinglées** ont donc un budget total de **200 Mo**. Au-delà, les plus anciennes sont purgées en premier. Les favoris n'ont pas de limite. Cela s'ajoute à `prune`, avec ses tests.
- **Exception à « texte d'abord »** : si le presse-papiers contient une image **et** un format HTML réduit à une seule balise `<img>`, l'image l'emporte sur le texte. C'est le cas de « Copier l'image » dans un navigateur qui ajoute l'URL en texte, par exemple Firefox ; à vérifier pendant la construction. Excel et Word posent un HTML de tableau ou de paragraphe, donc restent du texte.
- **Taille de l'image dans l'éditeur** : l'image est affichée ajustée à la zone, mais l'enregistrement se fait à la **résolution d'origine**. Les coordonnées des annotations sont converties en conséquence.

## Laissé au constructeur

- La forme exacte du schéma et de la migration.
- La façon de transmettre miniatures et images au frontend (commande qui renvoie des octets ou une data URL, protocole personnalisé…), tant que la liste ne charge que les miniatures.
- Épaisseur des traits, police et taille du texte, style de la pointe de flèche et des pastilles d'étape, à accorder avec le thème sombre/clair existant.
- Détection d'une image dans le presse-papiers (`arboard` `get_image`, liste de fichiers via `clipboard-win` ou `arboard`), dans le respect de la règle texte d'abord.
- Les tests Rust à ajouter dans `db.rs` : insertion d'une image, dédoublonnage, purge, remplacement après annotation, export/import v2 et v1.
