import { useState } from 'react';
import './VirtualDeviceModal.css';

/**
 * VirtualDeviceModal — first-run setup for the Nodus virtual device (R17).
 *
 * Human tone, no jargon up front: one sentence + two choice cards (auto-install
 * / later) and a quiet "skip". The technical bits (Test Mode, signing) hide
 * under a "details" spoiler. Visual only — the real install is wired in R18.
 */
export function VirtualDeviceModal({ onClose }: { onClose: () => void }) {
  const [details, setDetails] = useState(false);

  return (
    <div className="vdm-overlay" onClick={onClose}>
      <div className="vdm" role="dialog" aria-label="set up Nodus" onClick={(e) => e.stopPropagation()}>
        <div className="vdm-icon" aria-hidden>
          <WaveIcon />
        </div>
        <h2 className="vdm-title">One quick setup</h2>
        <p className="vdm-text">
          Nodus needs a virtual audio device so your apps can send sound through it. It takes a
          moment and you only do it once.
        </p>

        <div className="vdm-choices">
          <button className="vdm-choice is-primary" onClick={onClose}>
            <span className="vdm-choice-title">Install it for me</span>
            <span className="vdm-choice-sub">recommended · automatic</span>
          </button>
          <button className="vdm-choice" onClick={onClose}>
            <span className="vdm-choice-title">Set up later</span>
            <span className="vdm-choice-sub">use Nodus without it for now</span>
          </button>
        </div>

        <button className="vdm-details-toggle" onClick={() => setDetails((v) => !v)}>
          {details ? '▾' : '▸'} technical details
        </button>
        {details && (
          <p className="vdm-details">
            The driver is loaded once. On a development build Windows asks for Test Mode; release
            builds are signed so no extra steps are needed. Nothing is sent anywhere — the device
            lives only on your PC.
          </p>
        )}

        <button className="vdm-skip" onClick={onClose}>
          skip for now
        </button>
      </div>
    </div>
  );
}

function WaveIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2 12h3l2-7 4 18 3-13 2 6h6" />
    </svg>
  );
}
