import { useRef, Suspense } from 'react'
import { Canvas } from '@react-three/fiber'
import { useFrame } from '@react-three/fiber'
import { Shield } from './Shield'

// Six green point lights placed around the shield like a clock face.
// Because the model rotates, each facet sweeps through multiple light cones
// in sequence → more neon glints appear at different spots during rotation.
// Lights gently pulse at different phases so the highlights aren't all in
// sync (avoids a "breathing" look; keeps it organic).
const LIGHTS = [
  // [x, y, z,  baseIntensity, pulseAmp, pulseFreq, color]
  [ 4,  5,  4,  22, 5,  0.40, '#c8f9b6' ],  // top-right key
  [-4,  4,  4,  14, 4,  0.53, '#7fee64' ],  // top-left
  [ 4, -4,  4,  12, 3,  0.47, '#7fee64' ],  // bottom-right
  [-3, -3,  4,   9, 3,  0.61, '#7fee64' ],  // bottom-left fill
  [ 0,  0, -4,   8, 2,  0.35, '#7fee64' ],  // rim (behind)
  [ 0,  5, -3,   6, 2,  0.58, '#c8f9b6' ],  // top rim highlight
]

function AnimatedLights() {
  const refs = LIGHTS.map(() => useRef())

  useFrame(({ clock }) => {
    const t = clock.getElapsedTime()
    LIGHTS.forEach(([,, , base, amp, freq], i) => {
      if (refs[i].current)
        refs[i].current.intensity = base + Math.sin(t * freq + i * 1.1) * amp
    })
  })

  return (
    <>
      {LIGHTS.map(([x, y, z, base,,,color], i) => (
        <pointLight
          key={i}
          ref={refs[i]}
          position={[x, y, z]}
          intensity={base}
          color={color}
        />
      ))}
    </>
  )
}

export function ShieldScene({ active = true }) {
  return (
    <Canvas
      camera={{ position: [0, 0, 5], fov: 52 }}
      gl={{ antialias: true, alpha: true }}
      style={{ background: 'transparent' }}
      dpr={[1, 2]}
      frameloop={active ? 'always' : 'never'}
    >
      <ambientLight intensity={0.05} />
      {/* Muted overhead cone — illuminates the top facets of the shield */}
      <spotLight
        position={[0, 9, 2]}
        angle={0.45}
        penumbra={0.9}
        intensity={18}
        color="#3d6b38"
        decay={2}
      />
      <AnimatedLights />
      <Suspense fallback={null}>
        <Shield />
      </Suspense>
    </Canvas>
  )
}
