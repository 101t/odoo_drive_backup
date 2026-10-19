import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router";
import { UNAUTHORIZED_EVENT } from "./api/client";
import { keys, useAuth } from "./api/hooks";
import { Layout } from "./components/Layout";
import { useLocale, useT } from "./i18n";
import { Alert, LoadingBlock } from "./components/ui";
import { useLiveEvents } from "./lib/live";
import { DashboardPage } from "./pages/Dashboard";
import { DestinationsPage } from "./pages/Destinations";
import { InstanceDetailPage } from "./pages/InstanceDetail";
import { InstancesPage } from "./pages/Instances";
import { JobsPage } from "./pages/Jobs";
import { LoginPage } from "./pages/Login";
import { RunDetailPage } from "./pages/RunDetail";
import { RunsPage } from "./pages/Runs";
import { SettingsPage } from "./pages/Settings";

function AuthenticatedApp() {
  useLiveEvents(true);
  return (
    <Layout>
      <Routes>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/instances" element={<InstancesPage />} />
        <Route path="/instances/:id" element={<InstanceDetailPage />} />
        <Route path="/jobs" element={<JobsPage />} />
        <Route path="/runs" element={<RunsPage />} />
        <Route path="/runs/:id" element={<RunDetailPage />} />
        <Route path="/destinations" element={<DestinationsPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Layout>
  );
}

export function App() {
  // Re-render everything when the language changes.
  const locale = useLocale();
  const t = useT();
  const { data: auth, isLoading, error } = useAuth();
  const queryClient = useQueryClient();

  // Schedule descriptions come from the server in the UI language.
  useEffect(() => {
    queryClient.invalidateQueries({ queryKey: keys.jobs });
  }, [locale, queryClient]);

  useEffect(() => {
    const onUnauthorized = () => queryClient.invalidateQueries({ queryKey: keys.auth });
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
  }, [queryClient]);

  if (isLoading) return <LoadingBlock />;
  if (error || !auth) {
    return (
      <div className="mx-auto max-w-md p-8">
        <Alert tone="red" title={t("app.unreachable")}>
          {error instanceof Error ? error.message : t("app.unknown_error")}
        </Alert>
      </div>
    );
  }

  return (
    <BrowserRouter>
      {auth.user ? <AuthenticatedApp /> : <LoginPage setup={auth.setup_required} />}
    </BrowserRouter>
  );
}
