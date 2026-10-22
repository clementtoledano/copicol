# Journal des modifications

Notes de version de copicol. Chaque entrée liste les changements visibles
pour l'utilisateur ; les détails techniques restent dans l'historique Git.

## 0.5.0

### Images

- Les images copiées (captures d'écran, « Copier l'image » d'un navigateur, fichier image copié depuis l'Explorateur) apparaissent dans l'historique avec une miniature, leurs dimensions et leur poids ; un onglet « Images » les filtre
- Recollage d'une image comme une vraie image (Teams, Word, Outlook, Paint…), mise en favori, nommage, dossiers, export et import
- Éditeur d'annotation (`Ctrl+E`, bouton « Annoter » ou « Modifier » d'un favori) : flèche, ligne, rectangle, ellipse, étapes numérotées, crayon, texte, 5 couleurs, annuler/rétablir. « Enregistrer » remplace l'image, la place dans le presse-papiers et cache la fenêtre
- Les images non épinglées sont limitées à 200 Mo au total ; les plus anciennes sont supprimées en premier

## 0.4.0

### Interface

- Icônes vectorielles à la place des emoji, colorées par catégorie détectée
- Aperçus de l'historique limités à 3 lignes pour une liste plus lisible
- Actions (épingler, supprimer) déplacées dans le flux, ne recouvrent plus le texte
- Fusion des vues Favoris et Dossiers, avec un choix de regroupement
- Animation d'apparition de la fenêtre au raccourci `Ctrl+Shift+V`
- États vides illustrés (historique, favoris, recherche sans résultat)
- Thème clair et mode compact, accessibles depuis le menu ☰
- Notes de version affichées avant l'installation d'une mise à jour, et
  consultables à tout moment via « Notes de version » dans le menu ☰

## 0.3.4

- Modale d'édition de contenu agrandie, avec barre d'outils et rechercher/remplacer
- Compteur de caractères sur les champs de nom

## 0.3.3

- Correction du démarrage automatique quand le chemin d'installation contient une espace
- Notification de mise à jour disponible même fenêtre cachée
- Publication automatique des releases

## 0.3.0

- Confirmation avant de quitter l'application
- Ouverture du dépôt du projet depuis la fenêtre « À propos »
- Choix d'un dossier par défaut pour les favoris

## 0.2.0

- Démarrage automatique avec Windows, activable/désactivable
- Favoris nommés avec onglet dédié et accès depuis le menu du tray
- Import/export des favoris au format JSON
