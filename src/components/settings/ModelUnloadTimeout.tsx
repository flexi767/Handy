import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { commands, type ModelUnloadTimeout } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

interface ModelUnloadTimeoutProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

interface TimeoutOption {
  value: ModelUnloadTimeout;
  label: string;
}

export const ModelUnloadTimeoutSetting: React.FC<ModelUnloadTimeoutProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, getSetting, updateSetting } = useSettings();

  const timeoutOptions: TimeoutOption[] = [
    {
      value: "never",
      label: t("settings.advanced.modelUnload.options.never"),
    },
    {
      value: "immediately",
      label: t("settings.advanced.modelUnload.options.immediately"),
    },
    {
      value: "min1",
      label: t("settings.advanced.modelUnload.options.min1"),
    },
    {
      value: "min2",
      label: t("settings.advanced.modelUnload.options.min2"),
    },
    {
      value: "min5",
      label: t("settings.advanced.modelUnload.options.min5"),
    },
    {
      value: "min10",
      label: t("settings.advanced.modelUnload.options.min10"),
    },
    {
      value: "min15",
      label: t("settings.advanced.modelUnload.options.min15"),
    },
    {
      value: "hour1",
      label: t("settings.advanced.modelUnload.options.hour1"),
    },
  ];

  const debugTimeoutOptions: TimeoutOption[] = [
    ...timeoutOptions,
    {
      value: "sec15",
      label: t("settings.advanced.modelUnload.options.sec15"),
    },
  ];

  const handleChange = async (newTimeout: ModelUnloadTimeout) => {
    try {
      await commands.setModelUnloadTimeout(newTimeout);
      updateSetting("model_unload_timeout", newTimeout);
    } catch (error) {
      console.error("Failed to update model unload timeout:", error);
    }
  };

  const currentValue = getSetting("model_unload_timeout") ?? "never";

  const options = useMemo(() => {
    return settings?.debug_mode === true ? debugTimeoutOptions : timeoutOptions;
  }, [settings]);

  return (
    <SettingContainer
      title={t("settings.advanced.modelUnload.title")}
      description={t("settings.advanced.modelUnload.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={options}
        selectedValue={currentValue}
        onSelect={(value) => {
          // Dropdown reports a plain string; map it back to the typed option
          // rather than casting, so an unknown value never reaches the backend.
          const selected = options.find(
            (candidate) => candidate.value === value,
          );
          if (selected) {
            handleChange(selected.value);
          }
        }}
        disabled={false}
      />
    </SettingContainer>
  );
};
