import { useRef, useMemo } from 'react'
import { useFrame } from '@react-three/fiber'
import { Float, Sparkles, useGLTF } from '@react-three/drei'
import * as THREE from 'three'

// Kick off GLB fetch before the Canvas mounts (no loader waterfall).
useGLTF.preload('/models/shield.glb')

// GLB bounds: X±0.807, Y±0.999, Z±0.110. Identity node transform, centered at origin.
// Scale 1.75 → ~3.5 units tall, filling ~72% of the 52-fov viewport at z=5.
const SCALE = 1.75

// ─── GLB shield body ─────────────────────────────────────────────────────────
function GLBBody() {
  const { scene } = useGLTF('/models/shield.glb')
  const matRef = useRef()

  const cloned = useMemo(() => {
    const c = scene.clone(true)
    const mat = new THREE.MeshPhongMaterial({
      color:     '#07110a',
      emissive:  '#0b1c0b',
      specular:  '#7fee64',
      shininess: 130,    // sharp highlights = crisp neon glints on rotation
    })
    matRef.current = mat
    c.traverse((child) => {
      if (child.isMesh) {
        child.material   = mat
        child.castShadow = false
        child.receiveShadow = false
      }
    })
    return c
  }, [scene])

  // No colour animation — pure green specular so the model stays dark with
  // selective neon glints. More light sources in ShieldScene provide the
  // "sparks on rotation" effect without muddying the hue.

  return (
    <primitive
      object={cloned}
      scale={[SCALE, SCALE, SCALE]}
    />
  )
}

// ─── Pulsing scan line sweeping top → bottom ──────────────────────────────────
function ScanLine() {
  const meshRef = useRef()
  const matRef  = useRef()
  useFrame(({ clock }) => {
    const t = clock.getElapsedTime()
    if (meshRef.current) meshRef.current.position.y = 1.85 - ((t * 0.7) % 3.8)
    if (matRef.current)  matRef.current.opacity = 0.20 + Math.sin(t * 2.8) * 0.07
  })
  return (
    <mesh ref={meshRef} position={[0, 1.85, 0.25]}>
      <planeGeometry args={[1.7, 0.006]} />
      <meshBasicMaterial
        ref={matRef}
        color="#7fee64"
        transparent
        opacity={0.20}
        blending={THREE.AdditiveBlending}
        depthWrite={false}
      />
    </mesh>
  )
}

// ─── Soft radial aura behind the model ───────────────────────────────────────
function Aura() {
  const ref = useRef()
  useFrame(({ clock }) => {
    if (ref.current)
      ref.current.material.opacity =
        0.055 + Math.sin(clock.getElapsedTime() * 0.65) * 0.022
  })
  return (
    <mesh ref={ref} position={[0, 0, -0.25]}>
      <planeGeometry args={[4.8, 5.2]} />
      <meshBasicMaterial
        color="#1a4020"
        transparent
        opacity={0.055}
        blending={THREE.AdditiveBlending}
        depthWrite={false}
      />
    </mesh>
  )
}

// ─── Thin concentric neon outlines matching the shield silhouette ─────────────
// Uses a procedural circle rings as stand-in corona rings (matches the
// reference image's concentric-ring motif; no UV needed).
function CoronaRing({ radius, opacity, z = 0.26 }) {
  const geo = useMemo(() => new THREE.RingGeometry(radius - 0.006, radius, 96), [radius])
  return (
    <mesh geometry={geo} position={[0, 0, z]}>
      <meshBasicMaterial
        color="#7fee64"
        transparent
        opacity={opacity}
        blending={THREE.AdditiveBlending}
        depthWrite={false}
        side={THREE.DoubleSide}
      />
    </mesh>
  )
}

// ─── Root export ──────────────────────────────────────────────────────────────
export function Shield() {
  const groupRef = useRef()

  useFrame(({ clock }) => {
    if (groupRef.current) {
      const t = clock.getElapsedTime()
      groupRef.current.rotation.y = t * 0.35
      groupRef.current.rotation.x = Math.sin(t * 0.28) * 0.04
    }
  })

  return (
    <Float speed={0.9} rotationIntensity={0} floatIntensity={0.35}>
      <group ref={groupRef}>
        <Aura />
        <GLBBody />
        <CoronaRing radius={1.85} opacity={0.10} />
        <CoronaRing radius={1.60} opacity={0.22} />
        <CoronaRing radius={1.35} opacity={0.14} />
        <CoronaRing radius={1.10} opacity={0.08} />
        <ScanLine />
        <Sparkles
          count={60}
          scale={[3.8, 4.5, 2.0]}
          size={1.1}
          speed={0.18}
          color="#7fee64"
          opacity={0.45}
        />
      </group>
    </Float>
  )
}
