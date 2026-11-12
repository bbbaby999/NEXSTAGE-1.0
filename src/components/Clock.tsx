import { useEffect, useState } from "react";
import { currentLang } from "../lib/i18n";

export default function Clock() {
  const [now, setNow] = useState(new Date());
  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 15000);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="clock">
      <strong>{now.toLocaleTimeString(currentLang(), { hour: "2-digit", minute: "2-digit" })}</strong>
      <small>{now.toLocaleDateString(currentLang(), { day: "numeric", month: "long", year: "numeric" })}</small>
    </div>
  );
}
