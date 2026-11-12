import { useEffect, useState } from "react";

export default function Clock() {
  const [now, setNow] = useState(new Date());
  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 15000);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="clock">
      <strong>{now.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" })}</strong>
      <small>{now.toLocaleDateString("pt-BR", { day: "numeric", month: "long", year: "numeric" })}</small>
    </div>
  );
}
