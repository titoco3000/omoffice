import { useEffect } from "react";
import { AuthProvider, useAuth } from "./auth/AuthContext";
import { ThemeProvider } from "./theme/ThemeContext";
import { RouterProvider, useLocation, navigate } from "./router/Router";
import { OrgSessionProvider } from "./org/OrgSessionContext";
import { MeetingLayoutProvider } from "./meeting/MeetingLayoutContext";
import NavContainer from "./layout/NavContainer";
import Home from "./pages/Home";
import Login from "./pages/Login";
import InPage from "./pages/InPage";
import Profile from "./pages/Profile";
import Org from "./pages/Org";
import OrgNew from "./pages/OrgNew";

const AUTHENTICATED_ROUTES = {
  "/in": InPage,
  "/profile": Profile,
  "/org": Org,
  "/org/new": OrgNew,
};

function RedirectTo({ to }) {
  useEffect(() => navigate(to), [to]);
  return null;
}

function AuthenticatedApp({ path }) {
  const Page = AUTHENTICATED_ROUTES[path];
  return (
    <OrgSessionProvider>
      <MeetingLayoutProvider>
        <NavContainer>{Page ? <Page /> : <RedirectTo to="/in" />}</NavContainer>
      </MeetingLayoutProvider>
    </OrgSessionProvider>
  );
}

function Pages() {
  const path = useLocation();
  const { token } = useAuth();

  if (path === "/") return <Home />;
  if (path === "/login") return token ? <RedirectTo to="/in" /> : <Login />;
  if (!token) return <Login />;
  return <AuthenticatedApp path={path} />;
}

export default function App() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <RouterProvider>
          <Pages />
        </RouterProvider>
      </AuthProvider>
    </ThemeProvider>
  );
}
