export function Footer() {
  return (
    <footer className="border-t border-[#3e4a3c]/40 py-8">
      <div className="mx-auto flex max-w-[1200px] flex-col items-center justify-between gap-4 px-6 sm:flex-row">
        <div className="flex items-center gap-2">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M12 2L4 6v6c0 4.418 3.358 8.564 8 9.93C16.642 20.564 20 16.418 20 12V6L12 2z" stroke="#7fee64" strokeWidth="1.5" strokeLinejoin="round" fill="none" />
            <path d="M9 12l2 2 4-4" stroke="#7fee64" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          <span className="font-mono text-xs text-[#677d64]">
            aegis — MIT License
          </span>
        </div>
        <p className="font-mono text-xs text-[#485346]">
          Built in Rust · Zero telemetry · Open source
        </p>
      </div>
    </footer>
  )
}
