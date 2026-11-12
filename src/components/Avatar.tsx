export default function Avatar({ name, src, size = 48 }: { name: string; src: string | null; size?: number }) {
  return (
    <div
      className="avatar"
      style={{ width: size, height: size, fontSize: size * 0.42, backgroundImage: src ? `url(${src})` : undefined }}
    >
      {!src && name.trim().charAt(0).toUpperCase()}
    </div>
  );
}
