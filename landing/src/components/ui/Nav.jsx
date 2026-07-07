import { useState } from 'react'

const NAV_LINKS = [
  { label: 'Why Aegis', href: '#why-aegis' },
  { label: 'How It Works', href: '#how-it-works' },
  { label: 'Audit Trail', href: '#audit-trail' },
]

export function Nav() {
  const [open, setOpen] = useState(false)

  return (
    <>
      <header className="fixed top-0 left-0 right-0 z-50 h-16 border-b border-[#3e4a3c]/60 bg-[#212525]/90 backdrop-blur-md">
        <div className="mx-auto flex h-full max-w-[1200px] items-center justify-between px-6">
          {/* Logo */}
          <a
            href="#"
            className="flex items-center gap-2.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7fee64] rounded"
          >
            <img
              src="/aegis.svg"
              width="30"
              height="30"
              alt=""
              aria-hidden="true"
              className="h-[30px] w-[30px] object-contain"
            />
            <span className="font-display text-base font-semibold uppercase tracking-wide text-[#ddffdc]">
              aegis
            </span>
          </a>

          {/* Nav links — desktop */}
          <nav className="hidden items-center gap-7 md:flex" aria-label="Main navigation">
            {NAV_LINKS.map(({ label, href }) => (
              <a
                key={label}
                href={href}
                className="font-body text-sm text-[#677d64] transition-colors duration-150 hover:text-[#ddffdc] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7fee64] rounded px-1"
              >
                {label}
              </a>
            ))}
          </nav>

          {/* Right cluster */}
          <div className="flex items-center gap-3">
            <a
              href="https://github.com/IliasAlmerekov/aegis"
              target="_blank"
              rel="noopener noreferrer"
              className="flex h-9 items-center gap-2 rounded px-4 text-sm font-medium text-[#000000] transition-colors duration-150 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7fee64] focus-visible:ring-offset-2 focus-visible:ring-offset-[#212525]"
              style={{ backgroundColor: '#7fee64' }}
              onMouseEnter={e => e.currentTarget.style.backgroundColor = '#c8f9b6'}
              onMouseLeave={e => e.currentTarget.style.backgroundColor = '#7fee64'}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                <path d="M12 2C6.477 2 2 6.484 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0112 6.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0022 12.017C22 6.484 17.522 2 12 2z" />
              </svg>
              Install
            </a>

            {/* Hamburger — mobile only */}
            <button
              className={`burger flex h-9 w-9 items-center justify-center rounded text-[#677d64] transition-colors duration-150 hover:text-[#ddffdc] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7fee64] md:hidden cursor-pointer${open ? ' is-open' : ''}`}
              onClick={() => setOpen(o => !o)}
              aria-label={open ? 'Закрыть меню' : 'Открыть меню'}
              aria-expanded={open}
              aria-controls="mobile-nav"
            >
              <svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
                <line className="burger-blade burger-blade--top" x1="2" y1="5" x2="16" y2="5" />
                <line className="burger-blade burger-blade--mid" x1="2" y1="9" x2="16" y2="9" />
                <line className="burger-blade burger-blade--bot" x1="2" y1="13" x2="16" y2="13" />
              </svg>
            </button>
          </div>
        </div>
      </header>

      {/* Mobile nav panel */}
      <div
        id="mobile-nav"
        className={`mobile-nav-panel fixed left-0 right-0 z-40 overflow-hidden md:hidden${open ? ' is-open' : ''}`}
        style={{ top: '64px' }}
        aria-hidden={!open}
      >
        <div className="slash-edge" aria-hidden="true" />
        <div className="border-b border-[#3e4a3c]/60 bg-[#212525]/96 backdrop-blur-md px-6 py-4">
          <nav className="flex flex-col" aria-label="Mobile navigation">
            {NAV_LINKS.map(({ label, href }, i) => (
              <a
                key={label}
                href={href}
                onClick={() => setOpen(false)}
                className="mobile-nav-link flex items-center justify-between py-3.5 font-body text-sm text-[#aed2a4] hover:text-[#ddffdc] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64] rounded"
                style={{
                  borderBottom: i < NAV_LINKS.length - 1 ? '1px solid #3e4a3c40' : 'none',
                }}
              >
                {label}
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#485346" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M9 18l6-6-6-6" />
                </svg>
              </a>
            ))}
          </nav>
        </div>
      </div>
    </>
  )
}
