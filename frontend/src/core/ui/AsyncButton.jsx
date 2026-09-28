//! A button that runs an async function on click, showing a spinner and
//! disabling itself for as long as that call is in flight
import { forwardRef, useState } from "react";
import Spinner from "./Spinner";

const AsyncButton = forwardRef(function AsyncButton(
  { onClick, disabled, className = "", children, ...props },
  ref,
) {
  const [loading, setLoading] = useState(false);

  const handleClick = async (event) => {
    setLoading(true);
    try {
      await onClick(event);
    } finally {
      setLoading(false);
    }
  };

  return (
    <button
      {...props}
      ref={ref}
      type={props.type ?? "button"}
      onClick={handleClick}
      disabled={disabled || loading}
      className={`inline-flex items-center gap-1.5 ${className}`}
    >
      {loading && <Spinner />}
      {children}
    </button>
  );
});

export default AsyncButton;
