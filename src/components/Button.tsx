import { ButtonHTMLAttributes } from "react";

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "primary" | "ghost" | "danger";
};

/** Botão padrão do app. `data-focus` o torna alcançável pela navegação por foco. */
export default function Button({ variant = "default", className = "", type = "button", ...rest }: Props) {
  return <button data-focus type={type} className={`btn ${variant} ${className}`} {...rest} />;
}
