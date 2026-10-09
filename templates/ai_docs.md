# BornEngine — AI context reference for language models

This file summarizes BornEngine's public API and architectural decisions for assistants that write, review, or document games made with the engine. It reflects this repository's code; the package version prepared for this release is `0.16.1`. Always check `package.json`, exports, implementation, and examples before assuming that the version or behavior is still current.

## Rules for writing or reviewing code

1. **Use the current class-first API.** Do not generate the old BloomEngine API with global `initWindow`, `runGame`, `beginDrawing`, `loadTexture()`, or numeric handles. There are no compatibility aliases for it.
2. **Do not pass `Game` to assets.** Create `Texture`, `Model`, `Font`, `Sound`, `Music`, and related resources through `game.assets` or `scene.assets`. For example: `this.assets.loadTexture('assets/player.png')`. `Texture` has no public constructor.
3. **Pass `Game` only to systems that need its context.** These include `new Scene(game)`, `new PhysicsWorld(game, options)`, `new PhysicsWorld2D(game, options)`, and `new ColyseusClient(game, endpoint)`.
4. **Scene updates are explicit.** `Game.run()` advances the application loop and engine services, but does not call `game.scenes.update(dt)` or schedule `updateFixed()`. The subclass decides when to advance scenes and physics systems.
5. **Automatic scene rendering depends on the base method.** `Game.render()` draws the current scene. If a subclass overrides `render()`, call `super.render()` where scene components should be drawn.
6. **Distinguish runtime settings from build settings.** `GameOptions.renderMode` selects the runtime rendering path. The `[bornengine].native_profile` profile in `perry.toml` selects Rust features used by the BornEngine CLI in native builds.
7. **Check failures explicitly.** The engine uses values such as `isReady`, `isLoaded`, `error`, `null`, and results with `ok/status`. Read the API types and handle these results; do not assume every factory throws exceptions.
8. **Do not promise features just because an old spec mentions them.** To consider a feature public, confirm it is exported by `package.json`/`src/index.ts`, implemented under `src/`, and has a current usage path. `docs/design/` and `docs/superpowers/` contain historical proposals and plans.
9. **Do not invent Perry or platform capabilities.** Use `bornengine check main.ts` or the appropriate target checks before recommending syntax, dependencies, or APIs. Web, Apple, and scripting compatibility differences are described in this file and the platform pages.

## Architecture in one sentence

Game code uses TypeScript classes; Perry compiles it ahead of time and communicates with Rust layers through a private FFI. Shared Rust code lives in `native/shared/`; the `native/<platform>/` crates connect the runtime to the host. Classes, services, factories, and ownership are the game-facing API. Numeric handles and FFI functions are internal details and must not appear in public examples.

The package prepared for this release is version `0.16.1`. Current public exports are listed in `package.json` and the `src/index.ts` barrel. The stable module map appears below; confirm exports in those files before adding an import.

## Creating a game

A standalone application has one `Game` instance and can keep its main behavior in a subclass:

```ts
import { Colors, Game } from '@bornengine/engine';

class MyGame extends Game {
  constructor() {
    super({
      window: { title: 'My Game', width: 960, height: 540 },
      targetFps: 60,
      renderMode: '2d',
    });
  }

  protected override onStart(): void {
    // Create or load resources and activate the initial scene.
  }

  protected override loop(deltaTime: number): void {
    // deltaTime is measured in seconds.
    this.scenes.update(deltaTime);
  }

  protected override render(): void {
    this.renderer.clear(Colors.SNOW);
    super.render();
  }

  protected override onStop(): void {
    // Perform additional application cleanup before the Game shuts down.
  }
}

const game = new MyGame();
if (!game.isReady) console.error(game.error);
else game.run();
```

`Game` provides `window`, `renderer`, `input`, `audio`, `scenes`, `sceneGraph`, `mobile`, `ui`, `gui`, `debugUi`, `assets`, and `scripting`. `run()` manages standalone frames and shutdown; `stop()` requests an orderly shutdown; `dispose()` releases the runtime outside the normal loop. A host that already owns the window and scheduler uses `runFrame(deltaTime, callbacks)`. Only one native runtime may be active per process.

## Scenes, objects, and components

`Scene` is a gameplay-oriented scene and requires its owning `Game`. `GameObject` represents an entity, hierarchy, and transform; `GameComponent` provides attachable behavior. The main object/component hooks are `onAwake`, `onStart`, `update`, `fixedUpdate`, `render`, and `onDestroy`.

```ts
import { Game, GameObject, Scene } from '@bornengine/engine';

class Level extends Scene {
  constructor(game: Game) {
    super(game, { name: 'Level' });
    const player = new GameObject({ name: 'Player', position: { x: 100, y: 100, z: 0 } });
    this.add(player);
  }
}

// Inside a Game subclass:
// this.scenes.changeTo(new Level(this));
```

`game.scenes.changeTo(scene)` activates a scene. Replacing or unloading the previous scene runs its exit/disposal lifecycle and releases `scene.assets`, `scene.vfx`, and resources registered with `scene.own(resource)`. An object must belong to exactly one scene; a component is attached to exactly one object.

`game.scenes.update(dt)` advances the active scene. `game.scenes.render(renderer)` is called by the base `Game.render()` and draws the active or paused scene. `scene.camera2D`, `scene.viewport2D`, and `scene.bindCameraRig2D(rig)` configure the camera and viewport for rendering and coordinate conversion.

Visual components such as `SpriteRenderer`, `Tilemap`, and `ParticleEmitter2D` are drawn automatically by the scene. The scene collects enabled components on active objects, sorts them by `renderOrder`, and preserves insertion order for ties. When overriding `Game.render()`, clear the screen and call `super.render()` to keep this drawing behavior.

## Resources and ownership

### Assets

- `game.assets`: cache and lifetime shared by the application; released when the `Game` is disposed.
- `scene.assets`: resources scoped to a level; released when the scene unloads.
- `AssetGroup`: grouped preloads with progress, per-asset results, and cancellation.
- Common factories: `loadTexture(path)`, `loadModel(path)`, `loadFont(path, size)`, `loadSound(path)`, `loadMusic(path)`, `createMesh`, `createMaterial`, and `createRenderTexture`.

Factories may return `null`; loadable resources also expose `isLoaded` and `error`. The manager tracks identity and cache state, removes disposed resources from its inventory, and releases any resources it still owns when the scope ends. Use the same owning `Game` for related components and native resources.

Native development builds from `bornengine run` or `bornengine dev` watch successfully loaded file-backed textures, sounds, and music. Changes are debounced for about 120 ms and preserve resource handles. Texture dimensions must stay the same; decode errors keep the previous texture. Already-playing sound voices keep their original samples, and reloading active music restarts it from the beginning with volume and loop settings preserved. Set `BLOOM_NO_HOT_RELOAD=1` to disable watchers. Optimized builds, Web/WASM, and builds without the `hot-reload` feature do not watch file-backed assets.

```ts
const texture = this.assets.loadTexture('assets/player.png');
if (texture === null || !texture.isLoaded) {
  console.error(texture?.error || 'Could not load player texture');
}
```

`SpriteSheet` references a `Texture` but does not dispose it. Do not use `new Texture(game, path)`; the texture constructor is internal. Some semantic systems, rather than assets, do take `Game` explicitly, such as physics worlds and the Colyseus client.

### Effects

`scene.vfx.createParticleSystem(capacity, config)` and `createDecalSystem(capacity)` provide 3D VFX scoped to the scene lifetime. For 2D particles, attach `ParticleEmitter2D` to a `GameObject`; the scene updates, draws, and disposes its pool with the component. The default capacity is 256 (maximum 100,000); an emitter accepts 1 to 1,024 frames from a single `SpriteSheet`. `emissionRate` enables continuous emission with `play()`; `emitBurst(count, { position, direction })` emits a burst, and `stop()` stops new particles without removing live ones. Shapes are point/circle/box/cone; configure `lifetime`, `speed`, `startSize`, `endSize`, and `spin` ranges, along with `acceleration`, `drag`, RGBA colors in the 0–255 range, `direction`, `frameRate`, and `local`/`world` space. Defaults: 1 s lifetime, zero speed, size 8, upward direction (`{ x: 0, y: -1 }`, because 2D physics uses +Y downward), and local space. With `frameRate: 0`, each particle keeps the random frame selected at birth.

## 2D API

BornEngine's current development focus is 2D. Use `Vector2D` for 2D math and vectors. Many parameters and serialized data also accept structural `{ x, y }` objects; prefer `Vector2D` for gameplay values and operations.

`Vector2D` is a mutable value type with factories, getters, and static/instance operations. Instance methods: `clone`, `set`, `copy`; getters `magnitude`, `sqrMagnitude`, aliases `length`/`lengthSquared`, and `normalized`; operations `add`, `subtract`, `multiply`, `divide`, `scale`, `dotWith`, `crossWith`, `distanceTo`, `interpolatedTo`, `rotatedBy`, `clamped`, `clampedMagnitude`, `equals`, and `equalsApprox`. Static helpers: `zero`, `one`, `up`, `down`, `left`, `right`, `from`, `sum`, `difference`, `componentProduct`, `componentQuotient`, `scaled`, `normalize`, `magnitude`, `sqrMagnitude`, `dot`, `cross`, `distance`, `distanceSquared`, `min`, `max`, `clamp`, `clampMagnitude`, `lerp`, `lerpUnclamped`, `moveTowards`, `reflect`, `project`, `angle`, `signedAngle`, `rotate`, and `perpendicular`. Operations that produce a vector return a new instance; `set`/`copy` mutate the instance and return `this`. `angle` and `signedAngle` return degrees; `rotate` takes radians. `Vector2D.up()` means Cartesian +Y, while the engine's 2D physics uses positive Y downward; convert directions when crossing these conventions.

`AStarGrid2D(width, height)` creates an open uniform-cost grid with up to 1,000,000 cells. Mark obstacles with `setWalkable`, then call `findPath(start, goal, options)` for a route including both endpoints or `null`. Orthogonal movement is the default; diagonal steps cost sqrt(2), and corner cutting stays disabled unless enabled. Per-cell terrain weights are not supported.

`SeededRandom(seed)` provides deterministic `next()`, half-open float `range(min, max)`, and inclusive integer `integer(min, max)` values without global random state. `Noise2D(seed)` provides smooth normalized `sample(x, y)` and normalized `fractal(x, y, options)` values for procedural terrain and other fields.

### Sprites and animation

- `SpriteSheet(texture, options)` creates named or grid frames with margins/spacing, pivots, and trim data. The sheet does not own the texture.
- `SpriteRenderer extends GameComponent` draws a frame with size, pivot, tint, flips, and visibility; the object's world position, scale, and Z rotation are applied during drawing.
- `SpriteAnimation` contains keyframes, duration/FPS, markers, and a `loop`, `once`, or `ping-pong` mode.
- `SpriteAnimator` controls per-object clips and states with bool/number/trigger parameters, AND conditions, transitions in declared order, and optional crossfades.
- `onMarker`, `onComplete`, and `onStateChanged` are hooks for gameplay and VFX. `seek()` does not fire markers by default. A `dt` that crosses multiple frames preserves marker order.
- Calling `play()` again on the current clip does not restart it without `restart: true`; the default fade duration is zero.

Attach the renderer and animator to the same `GameObject`. The scene handles update and drawing while the object is active. The 3D model `Animation` API remains separate.

### 2D physics, tilemaps, and maps

`PhysicsWorld2D` implements a deterministic arcade solver in pixels/second, with positive Y downward. Call `step(deltaTime)` once per update; it accumulates time and runs substeps for `PhysicsBody2D` bodies. Dynamic bodies support axis-aligned boxes and circles; segments and convex polygons are static surfaces. Collider rotation/scaling, 2D joints, and dynamic polygon pairs are not part of this solver.

`CharacterBody2D.moveAndSlide()` is driven by the game. To dispatch `GameObject.fixedUpdate()`/`GameComponent.fixedUpdate()`, implement an accumulator and call `this.scenes.updateFixed(fixedDt)` on each fixed tick. `Game.run()` does not create this scheduler.

`Tilemap` is a scene component with tile and collision data. `World2DDocument` v1 is versioned JSON independent of 3D `WorldData`. Use `validateWorld2D`, `serializeWorld2D`, `World2DComponentRegistry`, and `World2DLoader`; the loader takes `resolveSpriteFrame` and, when the document contains bodies, a ready `PhysicsWorld2D` instance. The CLI importer `bornengine import tiled <map.tmx> --output <world.world2d.json>` accepts orthogonal Tiled maps within the documented finite-map subset.

There is no public `Navigation2D` class or weighted navigation mesh in this version. Use exported `AStarGrid2D` for uniform-cost grid pathfinding.

### Cameras and viewport

`CameraRig2D` is a component for follow, dead zones, bounds, zoom, and shake. `Viewport2D` defines a logical resolution and `fit`, `integer`, or `stretch` mode; `ParallaxLayer2D` applies parallax offsets. A `Camera2D` can also be configured directly on `scene.camera2D`.

## 3D and 2.5D API

3D remains available, but it is not the engine's main development focus at this time. `Model`, `Mesh`, `Material`, and `models.Animation` work with model assets. `game.sceneGraph` manages retained `SceneNode`s, hierarchy, geometry, materials, lights, and picking. `WorldData`, `WorldInstance`, and `PrefabLibrary` are 3D world data/runtime; they are not the World2D map format.

`PhysicsWorld(game, options)` uses the Jolt backend; connect the required adapters and call `world.step(dt)` from gameplay to synchronize `GameObject`s. Register a level's world with `scene.own(world)` to release it on unload. 3D VFX use `scene.vfx`; do not confuse the 3D `ParticleSystem` with the `ParticleEmitter2D` component.

`GameOptions.renderMode` accepts `2d`, `2.5d`, and `3d` and selects the runtime rendering path. To compile less Rust code for native targets, also use the CLI profile in `perry.toml`; `renderMode` alone does not remove executable features.

## Audio, input, UI, and debugging

- Audio: `game.audio` creates/loads `Sound` and `Music`; `AudioEmitter2D` is a positional 2D component. Audio services advance through the `Game` loop.
- Input: `game.input` reads keyboard/mouse/gamepad and supports `InputActionMap`. `game.mobile` provides virtual joysticks and buttons; `movementInput()` combines keyboard input, while gamepad/joystick input takes precedence above the deadzone.
- UI: `game.ui` is immediate-mode game UI; `game.gui` is the retained, object-oriented GUI tree. Keep both separate from diagnostic UI (`game.debugUi`).
- Debug: `game.debugUi` provides an inspector and Dear ImGui windows. It is opt-in and requires the `debug-ui` feature in native Linux/macOS/Windows builds. It is unavailable on Web, Apple mobile, and watchOS.

## Retained GUI controls

`game.gui` provides reusable, retained 2D controls; `game.ui` remains the immediate-mode API. Controls are `GUI` subclasses and can be extended or composed. Import them from `@bornengine/engine` or `@bornengine/engine/gui`, attach root controls with `game.gui.addControl(root)`, and attach children with `parent.addControl(child)`. A control belongs to at most one parent. `parent` and `getParent()` are read-only; `getControls()`, `getRoot()`, `removeControl()`, and `clearControls()` manage the tree.

Control positions are logical-pixel offsets relative to the parent content area. `GUIControlOptions.position` accepts `Vector2D` or any `{ x, y }` value; numeric `x`/`y` options remain supported and override their matching component. `getPosition()` returns a detached `Vector2D` snapshot. `setPosition(vector)` and `setPosition(x, y)` both work; `moveBy(offset)` translates both axes. `getX()`/`setX()`, `getY()`/`setY()`, `getSize()`/`setSize(width, height)`, `center()`, `centerHorizontal()`, and `centerVertical()` are available. `localToGlobal(point)` and `globalToLocal(point)` accept vector-like values and return `Vector2D` snapshots. Width and height stay independent dimensions.

```ts
import { GuiButton, GuiPanel, GuiScroll, Vector2D } from '@bornengine/engine';

class InventoryScroll extends GuiScroll {}

const panel = new GuiPanel({ position: new Vector2D(24, 24), width: 320, height: 240 });
const scroll = new InventoryScroll({ position: Vector2D.zero(), width: 280, height: 180 });
scroll.center().moveBy(Vector2D.right().scale(12));
scroll.addControl(new GuiButton({ position: new Vector2D(8, 8), width: 160, height: 32 }));
panel.addControl(scroll);
// Attach this root to the current Game instance.
game.gui.addControl(panel);
```

Built-in control families include layout (`GuiWindow`, `GuiPanel`, `GuiScroll`, `GuiBitmapBorder`, `GuiStretch`, `GuiFrameSet`), buttons and values (`GuiButton`, `GuiCheckBox`, `GuiRadioButton`, `GuiBitmapButton`, `GuiSlider`), text/editing (`GuiText`, `GuiMLText`, `GuiTextEdit`, `GuiMLTextEdit`, `GuiTextEditSlider`), selection/navigation (`GuiArray`, `GuiPopUpMenu`, `GuiPopUpEdit`, `GuiTreeView`, `GuiTextList`, `GuiTab`, `GuiMenu`, `GuiContextMenu`), and display (`GuiBitmap`, `GuiShowImg`, `GuiProgress`, `GuiDrawingPanel`). `GuiContextMenu.openAt(position, button)` accepts a vector-like position and keeps the numeric overload `openAt(x, y, button)`. Consult the controls guide for each class's methods and options.

`GuiProfile`/`GUIProfiles` configure colors, fonts, alignment, spacing, borders, opacity, shadows, focus, and cursor. Controls can share a profile or clone one with `setOwnProfile()`. Override hooks such as `onAction`, `onChange`, focus, pointer, and key callbacks; events bubble to parents and support `stopPropagation()`. Events expose `localPosition` and `globalPosition` as `Vector2D` snapshots while retaining scalar coordinate fields. Native responses are applied before the next `Game.loop()`; the GUI does not automatically consume game input, so check `game.gui.wantsPointerInput()` and `wantsKeyboardInput()`.

**watchOS:** retained GUI controls currently do not render and GUI input/events are unavailable. This is a temporary limitation; a future SwiftUI adapter is planned without a delivery date. Check `game.gui.isAvailable()` before relying on GUI behavior on a target.

## SQLite persistence

`GameDatabase` is a typed SQLite layer with explicit schemas (`defineSchema`, `defineTable`, `columns`), migrations (`defineMigration`), CRUD, filters, and transactions. A TypeScript schema does not create tables by itself; provide ordered migrations. The API does not accept arbitrary SQL. Operations return `DatabaseResult`; check `ok` and `status`.

Persistent mode is the default on supported targets. On Web, each opened database has its own Worker; it uses OPFS when the required capability exists and IndexedDB snapshots when OPFS is unsupported. Browser quota and eviction remain possible. `inMemory: true` selects volatile storage. The SQLite file is not encrypted; do not store credentials in it. Close every database to release workers/locks.

## Colyseus multiplayer

`ColyseusClient(game, endpoint)` manages connections associated with the `Game`. Use authoritative servers and send player intent rather than a final position the server trusts. The engine synchronizes room data, but does not automatically create a `GameObject` for each remote entity; keep a view layer and synchronize it with snapshots.

`Game.run()` pumps the Colyseus service each frame. In native Perry games, the standalone loop blocks; use `joinOrCreateWithCallbacks()`/`requestWithCallbacks()` when promises need an event loop that is not being yielded to. An embedded host must keep calling `runFrame()`. To clean up explicitly, leave rooms and dispose of the client.

## Scripting sandbox

`game.scripting` creates a `ScriptRuntime`; `ScriptComponent` receives a self-contained JS source string and is attached to a `GameObject`. Each component uses an isolated QuickJS runtime/heap. Permissions are denied by default and granted explicitly: `log`, `self.read`, `self.transform.write`, and `self.particles.emit`.

The guest module is a self-contained JS string with `export default { onStart(ctx), update(ctx, deltaTime), onDestroy(ctx) }`; all hooks are optional and synchronous. `ctx.log` requires `log`; `ctx.self.id/position` requires `self.read`; `ctx.self.setPosition/moveBy` requires `self.transform.write`; `ctx.particles.emitBurst(count, directionX?, directionY?)` requires `self.particles.emit` and a `ParticleEmitter2D` attached to the same object. Commands are applied after a synchronous hook finishes; an error or a returned Promise fails the hook and discards commands queued by that hook. Guest scripts do not receive `Game`, renderer, native handles, modules, filesystem, network, workers, Node APIs, or QuickJS `std`/`os`. Errors are exposed through `status/error`. The source limit is 1 MiB; default per-component limits are 16 MiB heap, 256 KiB stack, and 10,000 interrupt checks per hook. Accepted ranges are 64 KiB–64 MiB heap, 16–256 KiB stack, and 1–1,000,000 checks. The runtime does not load script files or manifests automatically. The host must load or embed the source; the CLI currently has no `bornengine script check` or `bornengine script pack` command.

Validated v1 targets: native Linux and Web/WASM. Other native targets and watchOS must be checked with `game.scripting.isSupported`; do not claim support for them. This isolation is a containment layer for game content, not a formal security boundary against VM vulnerabilities or modified multiplayer clients.

## Native profiles and CLI

`bornengine create` is interactive and asks for a name, game type, package manager, and stable engine version. `new` is the named/flagged variant; `init` initializes the current directory.

```sh
bornengine new MyGame --game-type 2d --package-manager pnpm --engine-version 0.15.0
bornengine check main.ts
bornengine run main.ts
bornengine dev main.ts --watch
bornengine build main.ts --os linux
bornengine --add-ai-docs assistant-guide
```

`--game-type` accepts `2d`, `2.5d`, and `3d` (alias `--kind`) and writes `[bornengine].native_profile` in `perry.toml`. The CLI applies it to the engine's Rust crate in native builds:

| Profile | Main features | Use |
| --- | --- | --- |
| `2d` | `mp3` | 2D renderer without Jolt or the 3D model loader |
| `2.5d` | `mp3`, `models3d`, `image-extras` | 3D models without Jolt |
| `3d` | `mp3`, `jolt`, `models3d`, `image-extras` | 3D models and Jolt physics |

Extra features can be added to `[bornengine].native_features`, for example `debug-ui` in Linux/macOS/Windows builds. Native `bornengine run` and `bornengine dev` development builds also enable the `hot-reload` and `dev` features; `bornengine build` and commands passed `--release` keep those development-only features disabled. Set `BLOOM_NO_HOT_RELOAD=1` to disable asset and material file watchers at runtime. Direct Perry commands do not read the BornEngine profile; Web uses a precompiled WASM artifact and does not watch files. `bornengine dev --watch` separately watches source and asset directories to rebuild and restart the game.

`bornengine create` and `bornengine new` include this guide in the generated project's root as `AGENTS.md`. To copy it into the current directory under another name, run `bornengine --add-ai-docs <filename>`; the CLI appends `.md` when needed and does not overwrite an existing file.

Do not confuse engine commands with CLI updates:

- `bornengine engine install [version]`, `engine update`, `engine use`, and `upgrade [version]` change the project's BornEngine dependency and lockfile.
- `bornengine engine list` lists releases available from the registry; it is not a list of engines installed in a local store.
- `bornengine update` only checks for a CLI release and prints install instructions; it does not update the executable.
- `bornengine perry install [--release <tag>]` downloads the Perry compiler for this host from a BornEngine release and verifies its SHA-256. `bornengine perry path`, `perry list`, and `perry clean [--dry-run]` show or remove managed Perry compilers.
- `BORNENGINE_PATH` and `--engine-path` are inputs to `new`/`init`; `engine use <path>` takes its path positionally.
- The CLI has no `bornengine script check` or `bornengine script pack` command.

## Targets and important differences

| Target | Notes |
| --- | --- |
| Linux / Windows / macOS | Native games; the profile controls Cargo features for the engine. `debug-ui` exists only in these three crates. |
| Android / iOS / tvOS / visionOS / watchOS | Requires platform-specific toolchains and configuration. Consult platform pages before claiming compatibility; optional features vary. |
| Web/WASM | Perry + engine WASM + JS glue. Requires WebGPU and a usable adapter; the current bootstrap does not fall back to WebGL. Published WASM is not profile-pruned by the CLI. |

This reference does not maintain a general browser support matrix. WebGPU depends on the browser, operating system, GPU, and hardware acceleration; validate a real adapter on the target.

## Package import map

The root import `@bornengine/engine` is appropriate for game code. Public subpaths for separating modules:

| Subpath | Area |
| --- | --- |
| `@bornengine/engine/core` | `Game`, `Window`, `Renderer`, platform types |
| `@bornengine/engine/game` | `GameObject`, `GameComponent`, `Scene`, `GameScene`, adapters |
| `@bornengine/engine/scene` | `SceneGraph`, `SceneNode` |
| `@bornengine/engine/assets` | `AssetManager`, `SceneAssetManager`, `AssetGroup` |
| `@bornengine/engine/textures` | `Texture`, `ImageData`, `RenderTexture` |
| `@bornengine/engine/sprites` | `SpriteSheet`, `SpriteRenderer`, `SpriteAnimation`, `SpriteAnimator`, `ParticleEmitter2D` |
| `@bornengine/engine/math` | `Vector2D`, `Vec3`, `Vec4`, `Quat`, `Matrix4`, `Mathf`, `Collision` |
| `@bornengine/engine/pathfinding2d` | `AStarGrid2D` |
| `@bornengine/engine/procedural` | `SeededRandom`, `Noise2D` |
| `@bornengine/engine/shapes` | Drawing and collision primitives |
| `@bornengine/engine/camera2d` | `CameraRig2D`, `Viewport2D`, parallax |
| `@bornengine/engine/physics2d` | `PhysicsWorld2D`, `PhysicsBody2D`, `CharacterBody2D` |
| `@bornengine/engine/tilemap` | `Tilemap` |
| `@bornengine/engine/world2d` | `World2DDocument`, validation, serialization, loader |
| `@bornengine/engine/models` | `Model`, `Mesh`, `Material`, 3D `Animation` |
| `@bornengine/engine/physics` | `PhysicsWorld`, colliders, rigid bodies, joints, vehicle |
| `@bornengine/engine/world` | `WorldData`, `WorldInstance`, prefab library |
| `@bornengine/engine/vfx` | 3D `ParticleSystem` and `DecalSystem` |
| `@bornengine/engine/text` | `Font` |
| `@bornengine/engine/audio` | `AudioSystem`, sounds, music, 2D emitters |
| `@bornengine/engine/input` | `InputSystem`, `InputActionMap` |
| `@bornengine/engine/mobile` | Virtual joysticks and touch buttons |
| `@bornengine/engine/ui` | Game UI |
| `@bornengine/engine/gui` | Retained GUI controls, profiles, and events |
| `@bornengine/engine/debug-ui` | Dear ImGui inspector |
| `@bornengine/engine/storage` | Typed SQLite `GameDatabase` |
| `@bornengine/engine/scripting` | `ScriptRuntime`, `ScriptComponent` |
| `@bornengine/engine/colyseus` | `ColyseusClient`, `Room` |

This map is a guide; confirm each name against the package's current exports.

## Examples and detailed documentation

Use examples from the repository as recipes that should compile with the current code. General index: [`examples/README.md`](examples/README.md).

- Quick start: [`quickstart`](webpage/src/content/docs/getting-started/quickstart.md)
- GameObjects and scenes: [`api/game`](webpage/src/content/docs/api/game.md)
- Ownership and preload: [`api/assets`](webpage/src/content/docs/api/assets.md)
- Complete 2D guide: [`guides/2d-game`](webpage/src/content/docs/guides/2d-game.md)
- Sprites/animation/particles: [`api/sprites`](webpage/src/content/docs/api/sprites.md)
- Retained GUI: [`api/gui`](webpage/src/content/docs/api/gui.md), [`guides/gui-controls`](webpage/src/content/docs/guides/gui-controls.md)
- 2D physics: [`api/physics2d`](webpage/src/content/docs/api/physics2d.md)
- World2D/Tiled: [`api/world2d`](webpage/src/content/docs/api/world2d.md), [`cli/import`](webpage/src/content/docs/cli/import.md)
- 3D: [`api/models`](webpage/src/content/docs/api/models.md), [`api/physics`](webpage/src/content/docs/api/physics.md)
- SQLite: [`api/storage`](webpage/src/content/docs/api/storage.md)
- Colyseus: [`guides/multiplayer`](webpage/src/content/docs/guides/multiplayer.md)
- Sandbox: [`api/scripting`](webpage/src/content/docs/api/scripting.md), [`examples/scripting-sandbox`](examples/scripting-sandbox/README.md)
- Targets: [`platforms`](webpage/src/content/docs/platforms/index.md)

To verify the API in the actual checkout, start with `package.json`, `src/index.ts`, the specific implementation under `src/<module>/`, `webpage/src/content/docs/`, and an example under `examples/`. If documentation and runtime disagree, treat current code, exports, and compile checks as the source of truth and fix the documentation.
