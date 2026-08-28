import { QRCodeSVG } from "qrcode.react";

export function JoinPanel({ code }: { code: string }) {
  const url = `${location.origin}/join/${code}`;
  return (
    <section className="join-panel">
      <div className="join-panel__qr">
        <QRCodeSVG value={url} size={260} bgColor="#ffffff" fgColor="#05070a" level="M" />
      </div>
      <div className="join-panel__text">
        <p className="join-panel__lead">Scan to join, or go to</p>
        <p className="join-panel__url">{location.host}/join</p>
        <p className="join-panel__lead">and enter</p>
        <p className="join-panel__code">{code}</p>
      </div>
    </section>
  );
}
