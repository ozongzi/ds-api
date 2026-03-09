import { createContext, useContext, useEffect, useReducer, type ReactNode } from "react";
import { api } from "../api/client";
import type { MeResponse } from "../api/types";

// ─── State ────────────────────────────────────────────────────────────────────

interface AuthState {
  token: string | null;
  user: MeResponse | null;
  loading: boolean;
}

type AuthAction =
  | { type: "SET_TOKEN"; token: string }
  | { type: "SET_USER"; user: MeResponse }
  | { type: "LOGOUT" }
  | { type: "SET_LOADING"; loading: boolean };

function reducer(state: AuthState, action: AuthAction): AuthState {
  switch (action.type) {
    case "SET_TOKEN":
      return { ...state, token: action.token };
    case "SET_USER":
      return { ...state, user: action.user, loading: false };
    case "LOGOUT":
      return { token: null, user: null, loading: false };
    case "SET_LOADING":
      return { ...state, loading: action.loading };
    default:
      return state;
  }
}

// ─── Context ──────────────────────────────────────────────────────────────────

interface AuthContextValue {
  token: string | null;
  user: MeResponse | null;
  loading: boolean;
  login: (token: string) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

const TOKEN_KEY = "familiar_token";

// ─── Provider ─────────────────────────────────────────────────────────────────

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, {
    token: localStorage.getItem(TOKEN_KEY),
    user: null,
    loading: true,
  });

  // On mount (or token change), fetch /api/users/me to validate the token.
  useEffect(() => {
    if (!state.token) {
      dispatch({ type: "SET_LOADING", loading: false });
      return;
    }

    let cancelled = false;
    dispatch({ type: "SET_LOADING", loading: true });

    api
      .getMe(state.token)
      .then((user) => {
        if (!cancelled) dispatch({ type: "SET_USER", user });
      })
      .catch(() => {
        if (!cancelled) {
          localStorage.removeItem(TOKEN_KEY);
          dispatch({ type: "LOGOUT" });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [state.token]);

  async function login(token: string) {
    localStorage.setItem(TOKEN_KEY, token);
    dispatch({ type: "SET_TOKEN", token });
    // SET_USER will be dispatched by the useEffect above.
  }

  async function logout() {
    if (state.token) {
      try {
        await api.logout(state.token);
      } catch {
        // best-effort
      }
    }
    localStorage.removeItem(TOKEN_KEY);
    dispatch({ type: "LOGOUT" });
  }

  return (
    <AuthContext.Provider
      value={{
        token: state.token,
        user: state.user,
        loading: state.loading,
        login,
        logout,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used inside <AuthProvider>");
  return ctx;
}
