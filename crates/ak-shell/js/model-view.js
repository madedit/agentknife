// The 3D model preview in the "File contents" tab: one model the user added
// (glTF, OBJ with its MTL, FBX), turning slowly until it is dragged. Uses the
// three.js that generated apps get (knowledge/tauri-leptos/libraries/three).
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { OBJLoader } from "three/addons/loaders/OBJLoader.js";
import { MTLLoader } from "three/addons/loaders/MTLLoader.js";
import { FBXLoader } from "three/addons/loaders/FBXLoader.js";

export class ModelView {
  // onState gets "loading <percent>", "ready" or "failed: <why>".
  constructor(canvas, url, ext, onState) {
    this.canvas = canvas;
    this.onState = onState;
    this.disposed = false;
    this.mixer = null;
    this.clock = new THREE.Clock();

    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    this.renderer.setPixelRatio(window.devicePixelRatio);
    this.scene = new THREE.Scene();
    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x666666, 2.5));
    const sun = new THREE.DirectionalLight(0xffffff, 2);
    sun.position.set(3, 5, 4);
    this.scene.add(sun);
    this.camera = new THREE.PerspectiveCamera(40, 1, 0.01, 1000);
    this.camera.position.set(2.4, 1.6, 2.8);
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = true;
    this.controls.autoRotate = true;
    this.controls.autoRotateSpeed = 1.5;
    this.onStart = () => {
      this.controls.autoRotate = false;
    };
    this.controls.addEventListener("start", this.onStart);

    this.resize = new ResizeObserver(() => this.fit());
    this.resize.observe(canvas);
    this.fit();
    this.renderer.setAnimationLoop(() => this.frame());
    this.load(url, ext.toLowerCase());
  }

  fit() {
    const w = this.canvas.clientWidth || 1;
    const h = this.canvas.clientHeight || 1;
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
  }

  frame() {
    this.mixer?.update(this.clock.getDelta());
    this.controls.update();
    this.renderer.render(this.scene, this.camera);
  }

  load(url, ext) {
    const progress = (e) => {
      if (e.total) this.onState(`loading ${Math.round((100 * e.loaded) / e.total)}`);
    };
    const failed = (err) => {
      if (!this.disposed) this.onState(`failed: ${err?.message ?? err}`);
    };
    const done = (object, animations) => this.show(object, animations ?? []);
    this.onState("loading 0");
    if (ext === "glb" || ext === "gltf") {
      new GLTFLoader().load(url, (gltf) => done(gltf.scene, gltf.animations), progress, failed);
    } else if (ext === "fbx") {
      new FBXLoader().load(url, (group) => done(group, group.animations), progress, failed);
    } else if (ext === "obj") {
      // The .mtl next to it, when there is one, gives the colors and pictures.
      const [path, query] = url.split("?");
      const mtl = path.replace(/\.obj$/i, ".mtl") + (query ? `?${query}` : "");
      const plain = () => new OBJLoader().load(url, (group) => done(group), progress, failed);
      new MTLLoader().load(
        mtl,
        (materials) => {
          materials.preload();
          new OBJLoader().setMaterials(materials).load(url, (group) => done(group), progress, failed);
        },
        undefined,
        plain,
      );
    } else {
      failed(`.${ext} files are not shown`);
    }
  }

  show(object, animations) {
    if (this.disposed) return;
    // Two units across, standing on a grid at the origin.
    const box = new THREE.Box3().setFromObject(object);
    const longest = Math.max(...box.getSize(new THREE.Vector3()).toArray()) || 1;
    object.scale.multiplyScalar(2 / longest);
    box.setFromObject(object);
    const center = box.getCenter(new THREE.Vector3());
    object.position.sub(new THREE.Vector3(center.x, box.min.y, center.z));
    this.scene.add(object);
    const grid = new THREE.GridHelper(4, 8, 0x888888, 0xcccccc);
    this.scene.add(grid);
    this.controls.target.set(0, (box.max.y - box.min.y) / 2, 0);
    if (animations.length > 0) {
      this.mixer = new THREE.AnimationMixer(object);
      this.mixer.clipAction(animations[0]).play();
    }
    this.onState("ready");
  }

  dispose() {
    this.disposed = true;
    this.renderer.setAnimationLoop(null);
    this.resize.disconnect();
    this.controls.removeEventListener("start", this.onStart);
    this.controls.dispose();
    this.mixer?.stopAllAction();
    this.scene.traverse((obj) => {
      obj.geometry?.dispose();
      for (const m of [obj.material].flat()) {
        for (const value of Object.values(m ?? {})) {
          if (value?.isTexture) value.dispose();
        }
        m?.dispose();
      }
    });
    this.renderer.dispose();
  }
}
