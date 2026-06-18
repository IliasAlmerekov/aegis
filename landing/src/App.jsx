import { Nav } from './components/ui/Nav'
import { Hero } from './components/sections/Hero'
import { FeatureSection } from './components/sections/FeatureSection'
import { HowItWorks } from './components/sections/HowItWorks'
import { TrustStrip } from './components/sections/TrustStrip'
import { AuditSection } from './components/sections/AuditSection'
import { CTABanner } from './components/sections/CTABanner'
import { Footer } from './components/sections/Footer'

function Divider() {
  return (
    <div
      aria-hidden="true"
      className="mx-auto max-w-[1200px] px-6"
    >
      <div className="h-px bg-[#3e4a3c]" />
    </div>
  )
}

export default function App() {
  return (
    <div className="min-h-dvh bg-[#000000] text-[#ddffdc]">
      <Nav />
      <main>
        <Hero />
        <Divider />
        <FeatureSection />
        <Divider />
        <HowItWorks />
        <TrustStrip />
        <AuditSection />
        <Divider />
        <CTABanner />
      </main>
      <Footer />
    </div>
  )
}
