# Skyblock Diorama — CPU Raytracer

Diorama de isla flotante estilo Minecraft, renderizado con un raytracer escrito desde
cero en Rust. **Todo el cómputo corre en CPU**: sin OpenGL, sin shaders, sin GPU.

La única dependencia externa es `minifb`, y sirve exclusivamente para mostrar en una
ventana el buffer que la CPU ya calculó (en Linux hace `XPutImage`, una copia de
píxeles; no crea contexto 3D). El motor también corre sin ventana con `--render`.

Escrito a mano, sin crates: matemática vectorial, decodificador y codificador PNG
(incluye inflate de deflate), lectura del ZIP del texture pack, ruido Perlin/fBm,
recorrido DDA de vóxeles, mapas normales y el planificador multihilo.

## Uso

```bash
cargo run --release                      # ventana interactiva
cargo run --release -- --render out/ -n 240   # órbita completa a PNGs, sin ventana
cargo run --release -- --bench 60        # medición de ms/frame
cargo test --release                     # pruebas unitarias
```

### Controles

| Tecla / acción | Efecto |
|---|---|
| Arrastrar mouse, flechas | Orbitar la cámara |
| Scroll, `W` / `S` | Acercar / alejar |
| `R` | Regenerar el terreno con otra semilla |
| `1`–`4` | Escala de resolución |
| `P` | Captura de pantalla a PNG |
| `Esc` | Salir |

## Texture pack

La escena usa **Ozocraft Remix** (32×32). El pack no se incluye en el repositorio por
tamaño y licencia. Para ejecutarlo, colocar el `.zip` del pack en:

```
texturepack/Ozocraft Remix 1.21+ [R17].zip
```

El programa lee las texturas directamente del ZIP con su propio inflate.
