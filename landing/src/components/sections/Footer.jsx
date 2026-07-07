export function Footer() {
  return (
    <footer className="border-t border-[#3e4a3c]/40 py-8">
      <div className="mx-auto flex max-w-[1200px] flex-col items-center justify-between gap-4 px-6 sm:flex-row">
        <div className="flex items-center gap-2">
          <img
            src="/aegis.svg"
            width="22"
            height="22"
            alt=""
            aria-hidden="true"
            className="h-[22px] w-[22px] object-contain"
          />
          <span className="font-mono text-xs text-[#677d64]">
            <span className="uppercase tracking-wide text-[#ddffdc]">aegis</span> — MIT License
          </span>
        </div>
        <p className="font-mono text-xs text-[#485346]">
          Built in Rust · Zero telemetry · Open source
        </p>
      </div>
    </footer>
  )
}
