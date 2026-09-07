# Skyblock Diorama — CPU Raytracer

Diorama de tres islas flotantes estilo Minecraft, renderizado con un raytracer escrito
desde cero en Rust. **Todo el cómputo corre en CPU**: sin OpenGL, sin shaders, sin GPU.

![Vista general](screenshots/overview.png)
![La puerta de cristal](screenshots/gate.png)
![La cabeza del dragón](screenshots/dragon.png)

Las dos islas vecinas, unidas a la principal por puentes de madera con barandas:

![La isla del Nether](screenshots/nether.png)
![La granja](screenshots/farm.png)

La hotbar de Minecraft hace de menú: se elige la ranura con los números y se usa
con `Enter`. Con `E` se abre un inventario de 32 bloques; el elegido va a la última
ranura y se pone en el mundo con click derecho, mientras el izquierdo quita el
bloque que haya bajo el puntero.

![La hotbar como menú](screenshots/hud.png)

Ciclo de día y noche — la misma escena a mediodía y a medianoche:

![Mediodía](screenshots/day.png)
![Medianoche](screenshots/night.png)

La única dependencia externa es `minifb`, y sirve exclusivamente para mostrar en una
ventana el buffer que la CPU ya calculó (en Linux hace `XPutImage`, una copia de
píxeles; no crea contexto 3D). El motor también corre sin ventana con `--render`.

Escrito a mano, sin crates: matemática vectorial, decodificador y codificador PNG
(incluye DEFLATE completo), lector del ZIP del texture pack, ruido Perlin/fBm,
recorrido DDA de vóxeles, mapas normales, skybox, y el planificador multihilo.

## Pantalla de inicio

Al abrir la ventana aparece una pantalla de título —fondo, créditos y dos botones,
**Jugar** y **Reglas**— dibujada con la misma tipografía del pack y el mismo
compositor a mano que la interfaz del juego. La escena no se construye hasta que se
presiona Jugar.

## Uso

```bash
cargo run --release                              # ventana interactiva
cargo run --release -- --render out/ -n 240 --samples 8   # órbita a PNGs, sin ventana
cargo run --release -- --bench 30                # medición de ms/frame
cargo run --release -- --bench-idle 60           # medición del frame en reposo
cargo run --release -- --bench-move 60           # medición del frame mientras se arrastra
cargo run --release -- --check-pack              # verifica la carga del texture pack
cargo test --release                             # 140 pruebas
```

Opciones: `--width W --height H --seed N --threads N --samples N --sky-panorama
--time T --cycle --eye X,Y,Z --look YAW,PITCH --hud`.

`--eye` y `--look` plantan la cámara libre en un punto concreto para el render
offline, útil para capturas desde dentro de la escena o desde debajo de la isla.

`--time T` fija la hora (`0` amanecer, `0.25` mediodía, `0.5` atardecer, `0.75`
medianoche) y `--cycle` barre un día completo a lo largo de los frames de `--render`,
que es como se arma el video del ciclo.

### Controles

| Tecla / acción | Efecto |
|---|---|
| Mover el mouse | Girar la cámara: el **mouselook** está activo desde el primer frame |
| `Tab` | Soltar o retomar el mouselook |
| Arrastrar mouse | Girar la cámara arrastrando la escena, con mouselook apagado |
| Flechas | Girar la cámara (arriba mira hacia arriba) |
| `W` / `S` | Acercar-alejar (órbita) · avanzar-retroceder (vuelo) |
| `A` / `D` | Girar alrededor de la isla (órbita) · desplazarse a los lados (vuelo) |
| `Espacio` / `Shift` | Subir / bajar (vuelo) |
| `Ctrl` | Moverse más rápido |
| Scroll | Acercar / alejar |
| `1`–`8` | Elegir ranura de la hotbar |
| `Enter` | Usar el objeto elegido (mantener, en el reloj y la brújula) |
| `Q` | Arrancar / detener el **ciclo de día y noche** |
| `E` | Abrir / cerrar el **inventario** |
| Click izquierdo | Quitar el bloque apuntado (la cruz, o el puntero sin mouselook) |
| Click derecho | Poner el bloque elegido contra la cara apuntada |
| `F` | Alternar entre **órbita** y **vuelo libre** |
| `H` | Mostrar u ocultar la hotbar |
| `,` / `.` | Mover la hora a mano |
| `R` | Regenerar el terreno con otra semilla |
| `F1`–`F4` | Escala de resolución directa (el telescopio la cicla) |
| `P` | Captura de pantalla a PNG |
| `Esc` | Cierra el inventario; luego suelta el mouse; luego sale |

La hotbar es el menú, y cada objeto es su acción:

| Ranura | Objeto | Qué hace |
|---|---|---|
| 1 | Pico de diamante | Cambia entre órbita y vuelo libre |
| 2 | Reloj | Mantener `Enter`: adelanta la hora (la esfera sigue al sol) |
| 3 | Brújula | Mantener `Enter`: regresa la hora (la aguja sigue a la cámara) |
| 4 | Cuadro | Guarda una captura PNG |
| 5 | Semillas | Genera otro terreno |
| 6 | Telescopio | Sube la escala de resolución: 1 → 2 → 3 → 4 → 1 |
| 7 | Barrera | Salir |
| 8 | Bloque | El que se eligió en el inventario (`E`); click derecho lo pone |

Mientras la cámara se mueve se traza **medio frame en tablero de ajedrez a resolución
completa** y la otra mitad se **interpola de sus dos vecinos de ese mismo frame**:
mismo costo que media resolución, sin bordes escalonados y sin arrastre —conservar el
píxel del frame anterior se veía como desenfoque de movimiento—. Si aun así un frame
pasa de 45 ms (una ventana grande), el arrastre baja a media resolución hasta que la
cámara se detiene. Al soltarla se **acumulan muestras jittereadas** hasta
converger, que es de donde sale el antialiasing; ya convergida solo se re-trazan los
píxeles que siguen cambiando —el agua, el portal y sus reflejos—.

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
| Dos islas vecinas y sus puentes | `neighbours.rs` — mismo ruido a menor escala, isla del Nether e isla granja |
| Complejidad de escena | `structures.rs` — templo, dragón enrollado, gran árbol, puente, ruina |
| Bloques parciales (slabs, cercas, cultivos) | `world.rs` — cajas dentro del vóxel resueltas dentro del propio DDA |
| Rotación y zoom de cámara | `camera.rs` — órbita con pitch acotado y zoom multiplicativo, más vuelo libre con `F` |
| 5 materiales con textura y parámetros propios | `assets.rs` — terreno, agua, metal, emisivo, cristal |
| Refracción | `render.rs` — Snell con reflexión interna total; agua y puerta de cristal |
| Reflexión | `render.rs` — Fresnel de Schlick; oro, hierro, diamante, agua |
| Mapas normales | `texture.rs` — derivados por Sobel de la luminancia de cada textura |
| Material emisivo | glowstone y la puerta, como luces puntuales reales |
| Skybox | `skybox.rs` — cielo procedural que sigue la hora (por defecto) o cubemap del panorama |
| Ciclo día/noche | `daylight.rs` — sol, luna, paleta del cielo y ambiente desde un solo número; tecla `Q` |
| Paralelismo y optimización | `parallel.rs`, tabla de mediciones abajo |

## La escena

Tres islas. La central sigue el diorama de referencia: isla flotante con un **templo de columnas** y su
puerta de cristal iluminada, un **dragón blanco y rojo enrollado alrededor del
templo** —lana blanca, cresta de lana y concreto rojo, espinas negras, garganta de
netherrack y ojos de glowstone— que da vuelta y media a la columnata y saca la
cabeza por encima de la cumbrera, un **gran árbol** sobre un afloramiento rocoso, un **río**
que cruza la isla y cae por los dos bordes, un **puente de piedra** con linternas, y
una **ruina** de columnas rotas en primer plano. Debajo, vetas de oro, hierro y
diamante que solo se ven al orbitar por abajo.

A los lados, dos islas más pequeñas generadas con el mismo ruido, unidas a la
principal por **puentes de madera** con cubierta de tablones, borde de *slabs*,
barandas de cercas y postes colgando al vacío:

- **Isla del Nether** (oeste): netherrack y arena de almas, una poza de **lava**,
  magma brillando por el envés de la isla, un arco roto de ladrillo del Nether y un
  **portal al Nether** de obsidiana, transparente, refractante y encendido.
- **Granja** (este): casa de tablones con esquinas de tronco, ventanas de vidrio y
  techo de *slabs*; un campo de **calabazas** en surcos sobre tierra de labor regada por
  un canal; pilas de **heno**; y un corral de cercas con una vaca y dos ovejas
  construidas con bloques.

## Rendimiento

Escena completa (isla 48×48), 900×600, CPU de 8 núcleos / 16 hilos:

| Hilos | ms/frame | fps | Aceleración |
|---|---|---|---|
| 1 | 239.85 | 4.2 | 1.00× |
| 4 | 63.92 | 15.6 | 3.75× |
| 16 | 27.03 | 37.0 | **8.55×** |

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

El ciclo de día y noche cuesta **~4.5 ms cada vez que rehornea el cielo** (medido con
`--bench N --cycle`, que fuerza un rehorneado por frame: 18.49 → 23.0 ms), pero lo caro
no es el cielo sino **la luz**: cada cambio de iluminación re-sombrea todos los píxeles
a resolución plena. Por eso la iluminación avanza en **240 pasos por día** y el día dura
**2 minutos**: son ~2 re-sombreados por segundo, y los otros ~58 frames refinan la
imagen en vez de recalcularla. El reloj sigue corriendo suave entre paso y paso.

Los dos caminos interactivos cuestan bastante menos que un frame completo:

| Camino (900×600, 16 hilos) | ms/frame | píxeles trazados |
|---|---|---|
| Frame completo | 29.6 | 100% |
| Arrastrando (tablero + interpolación) | **17.2** (58 fps) | 50% |
| Arrastrando, si el frame pasa de 45 ms | media resolución | 25% |
| En reposo (refresco selectivo) | **17.7** | 48% |

Un cambio de hora no reinicia nada: marca todos los píxeles como activos y re-sombrea
a resolución plena sobre la imagen que ya había, así el ciclo se ve como un fundido.

Reproducible con `--bench N` y `--bench-idle N`, más `--width W --height H
--threads N [--cycle]`.

## Estructura

```
src/
  main.rs        modos: ventana, render offline, benchmark, verificación del pack
  output.rs      trait Output: ventana o PNGs; el renderer no sabe cuál
  window.rs      único módulo que toca minifb
  splash.rs      pantalla de título: fondo, créditos, botones y reglas
  hud.rs         hotbar, iconos y tipografía del pack, compuestos sobre el frame
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
  daylight.rs    ciclo día/noche: sol, luna, paletas de cielo y ambiente
  terrain.rs     generación procedural de la isla, el montículo y el río
  structures.rs  templo, dragón, gran árbol, puente, ruina y las luces
  neighbours.rs  composición del mundo: isla del Nether, granja y los puentes
  world.rs       grid de vóxeles + DDA
  camera.rs      cámara orbital
  skybox.rs      cielo procedural y cubemap
  scene.rs       mundo + assets + iluminación + reloj de animación
  parallel.rs    reparto dinámico de trabajo entre hilos
  render.rs      sombreado, recursión y pipeline de frame
```
