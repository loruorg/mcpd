defmodule Mcpd.MixProject do
  use Mix.Project

  # The SDK encodes and decodes with `:json`, which OTP 27 added. On an older
  # OTP everything compiles and the first batch fails at run time with an
  # undefined function, inside the sender, where nobody is looking. Refusing
  # here turns that into one line at build time that says what to install.
  # The Elixir floor below is 1.17, the first release whose compatibility
  # table includes OTP 27.
  @otp_release :erlang.system_info(:otp_release) |> List.to_integer()
  if @otp_release < 27 do
    Mix.raise(
      "mcpd needs Erlang/OTP 27 or newer, for the :json module it encodes " <>
        "with. This is OTP #{@otp_release}. Install OTP 27 or newer, with " <>
        "Elixir 1.17 or newer, and build again."
    )
  end

  def project do
    [
      app: :mcpd,
      version: "0.1.0",
      elixir: "~> 1.17",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      description: "Agent observability and durable runs for Elixir. Sends traces, logs, metrics, scores and feedback to an mcpd daemon, and serves workflows from your own node.",
      package: package(),
      elixirc_options: [warnings_as_errors: true]
    ]
  end

  # `:inets` and `:ssl` for HTTP, `:crypto` for ids. All three ship with OTP.
  def application do
    [extra_applications: [:logger, :inets, :ssl, :crypto]]
  end

  # No dependencies, deliberately. OTP 27 carries a JSON codec and an HTTP
  # client, so an observability library does not need to drag anything into a
  # user's dependency tree to do its job. A tool you add to see your system
  # should not be the reason your system's build changes.
  defp deps, do: []

  defp package do
    [
      licenses: ["Apache-2.0"],
      links: %{"Source" => "https://github.com/mcpd/mcpd"}
    ]
  end
end
