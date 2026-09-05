# Skyblock Diorama — CPU Raytracer

Diorama de isla flotante estilo Minecraft, renderizado con un raytracer escrito desde
cero en Rust. **Todo el cómputo corre en CPU**: sin OpenGL, sin shaders, sin GPU.

![Vista general](screenshots/overview.png)
![La puerta de cristal](screenshots/gate.png)

La única dependencia externa es `minifb`, y sirve exclusivamente para mostrar en una
ventana el buffer que la CPU ya calculó (en Linux hace `XPutImage`, una copia de
píxeles; no crea contexto 3D). El motor también corre sin ventana con `--render`.

Escrito a mano, sin crates: matemática vectorial, decodificador y codificador PNG
(incluye DEFLATE completo), lector del ZIP del texture pack, ruido Perlin/fBm,
recorrido DDA de vóxeles, mapas normales, skybox, y el planificador multihilo.

## Uso

```bash
cargo run --release                              # ventana interactiva
cargo run --release -- --render out/ -n 240 --samples 8   # órbita a PNGs, sin ventana
cargo run --release -- --bench 30                # medición de ms/frame
cargo run --release -- --check-pack              # verifica la carga del texture pack
cargo test --release                             # 85 pruebas
```

Opciones: `--width W --height H --seed N --threads N --samples N --sky-panorama`.

### Controles

| Tecla / acción | Efecto |
|---|---|
| Arrastrar mouse, flechas | Orbitar la cámara |
| Scroll, `W` / `S` | Acercar / alejar |
| `R` | Regenerar el terreno con otra semilla |
| `1`–`4` | Escala de resolución |
| `P` | Captura de pantalla a PNG |
| `Esc` | Salir |

Mientras la cámara se mueve, el render baja de resolución para mantener la
interacción fluida; al soltarla vuelve a resolución completa y **acumula muestras
jittereadas** hasta converger, que es de donde sale el antialiasing.

## Texture pack

La escena usa **Ozocraft Remix** (32×32). El pack no se incluye en el repositorio por
tamaño y licencia. Para ejecutarlo, colocar el `.zip` del pack en:

```
texturepack/Ozocraft Remix 1.21+ [R17].zip
```

El programa lee las texturas directamente del ZIP con su propio DEFLATE.

## Qué implementa

| Elemento | Dónde |
|---|---|
| Terreno procedural 48×48 (supera el mínimo de 16×16) | `terrain.rs` — Perlin/fBm propio, isla con estalactita, montículo rocoso, río con dos cascadas, vetas de mineral, árboles |
| Complejidad de escena | `structures.rs` — templo, gran árbol, puente, ruina |
| Rotación y zoom de cámara | `camera.rs` — cámara orbital con pitch acotado y zoom multiplicativo |
| 5 materiales con textura y parámetros propios | `assets.rs` — terreno, agua, metal, emisivo, cristal |
| Refracción | `render.rs` — Snell con reflexión interna total; agua y puerta de cristal |
| Reflexión | `render.rs` — Fresnel de Schlick; oro, hierro, diamante, agua |
| Mapas normales | `texture.rs` — derivados por Sobel de la luminancia de cada textura |
| Material emisivo | glowstone y la puerta, como luces puntuales reales |
| Skybox | `skybox.rs` — cielo de atardecer procedural (por defecto) o cubemap del panorama |
| Paralelismo y optimización | `parallel.rs`, tabla de mediciones abajo |

## La escena

Siguiendo el diorama de referencia: isla flotante con un **templo de columnas** y su
puerta de cristal iluminada, un **gran árbol** sobre un afloramiento rocoso, un **río**
que cruza la isla y cae por los dos bordes, un **puente de piedra** con linternas, y
una **ruina** de columnas rotas en primer plano. Debajo, vetas de oro, hierro y
diamante que solo se ven al orbitar por abajo.

## Rendimiento

Escena completa (isla 48×48), 900×600, CPU de 8 núcleos / 16 hilos:

| Hilos | ms/frame | fps | Aceleración |
|---|---|---|---|
| 1 | 239.85 | 4.2 | 1.00× |
| 4 | 63.92 | 15.6 | 3.75× |
| 16 | 28.07 | 35.6 | **8.55×** |

Optimizaciones aplicadas, cada una medida a 640×520 con 16 hilos:

| Cambio | ms/frame |
|---|---|
| Cielo analítico evaluado por rayo | 98.86 |
| Cielo horneado en tabla lat-long | 9.84 |
| *(escena completa: recursión + luces)* | 92.94 |
| Omitir el rayo de sombra en caras que no ven el sol | 18.01 |
| Agrupar bloques emisivos vecinos en una sola luz | 17.89 |
| Descartar linternas cuya contribución es despreciable | 17.12 |
| Seguir solo la rama dominante tras la primera división | **13.52** |

Reproducible con `--bench N --width W --height H --threads N`.

## Estructura

```
src/
  main.rs        modos: ventana, render offline, benchmark, verificación del pack
  output.rs      trait Output: ventana o PNGs; el renderer no sabe cuál
  window.rs      único módulo que toca minifb
  math.rs        Vec3, reflect, refract (Snell + TIR), Fresnel, gamma
  inflate.rs     DEFLATE (stored, Huffman fijo y dinámico) + zlib
  png.rs         encoder y decoder PNG propios
  zip.rs         lector del texture pack
  texture.rs     texturas en luz lineal, animación y normales por Sobel
  pack.rs        acceso al pack
  blocks.rs      ids de bloque
  material.rs    parámetros ópticos por material
  assets.rs      bloque → textura por cara → material
  noise.rs       Perlin 2D/3D y fBm
  terrain.rs     generación procedural de la isla, el montículo y el río
  structures.rs  templo, gran árbol, puente, ruina y las luces
  world.rs       grid de vóxeles + DDA
  camera.rs      cámara orbital
  skybox.rs      cielo procedural y cubemap
  scene.rs       mundo + assets + iluminación + reloj de animación
  parallel.rs    reparto dinámico de trabajo entre hilos
  render.rs      sombreado, recursión y pipeline de frame
```
