import { AuthProvider, useAuth } from "./auth/AuthContext";
import { RouterProvider, useLocation } from "./router/Router";
import Home from "./pages/Home";
import Login from "./pages/Login";
import Panels from "./pages/Panels";

function Pages() {
  const path = useLocation();
  const { token } = useAuth();

  if (path === "/login") return token ? <Panels /> : <Login />;
  if (path === "/panels") return token ? <Panels /> : <Login />;
  return <Home />;
}

export default function App() {
  return (
    <AuthProvider>
      <RouterProvider>
        <Pages />
      </RouterProvider>
    </AuthProvider>
  );
}
