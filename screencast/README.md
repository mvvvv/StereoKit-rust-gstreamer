# Screencast

Application de capture d'écran (screencast) utilisant **ashpd** (portail XDG Desktop Portal) pour Wayland et ximagesrc pour X11.

## Caractéristiques

- ✅ Support Wayland via **ashpd** (portail XDG)
- ✅ Support X11 via ximagesrc
- ✅ Persistance de session (restore_token)
- ✅ Encodage H264, H265, VP9
- ✅ Support NVIDIA GPU et CUDA
- ✅ Mode RTSP ou UDP
- ✅ Interface graphique simple avec egui

## Avantages par rapport à screencast-vr

1. **Simplicité** : Utilise `ashpd` qui gère toute la complexité de D-Bus
2. **Moins de code** : ~400 lignes vs ~760 lignes
3. **Type-safe** : ashpd fournit des types Rust idiomatiques
4. **Maintenance** : ashpd est maintenu activement et suit les évolutions des portails

## Compilation

```bash
cargo build -p screencast
```

## Exécution

```bash
# Avec logs détaillés
RUST_LOG=debug cargo run -p screencast

# Sans logs
cargo run -p screencast
```

## Configuration

Le restore_token est sauvegardé dans :

- Linux: `~/.config/screencast/session_config.txt`
- macOS: `~/Library/Application Support/screencast/session_config.txt`
- Windows: `%APPDATA%\screencast\session_config.txt`

## Utilisation

1. Sélectionner l'encodage (H264, H265, VP9)
2. Choisir GPU/CUDA selon votre matériel
3. Configurer l'adresse de destination
4. Cliquer sur "Start ScreenCast"

## Dépendances

- `ashpd` : Interface Rust pour les portails XDG Desktop
- `eframe/egui` : Interface graphique
- `gstreamer` : Pipeline de streaming
- `tokio` : Runtime asynchrone
