import { fireEvent, render, screen } from "@testing-library/react";
import { InlineEdit, Switch } from "./ui";
import { TierBadge, FitBadge } from "./domain";

describe("ui", () => {
  it("InlineEdit renames on double-click + Enter", () => {
    const onSave = vi.fn();
    render(<InlineEdit value="Test writer" onSave={onSave} />);
    fireEvent.doubleClick(screen.getByText("Test writer"));
    const input = screen.getByDisplayValue("Test writer");
    fireEvent.change(input, { target: { value: "Unit test writer" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSave).toHaveBeenCalledWith("Unit test writer");
  });

  it("InlineEdit cancels on Escape", () => {
    const onSave = vi.fn();
    render(<InlineEdit value="Rule" onSave={onSave} />);
    fireEvent.keyDown(screen.getByText("Rule"), { key: "F2" });
    fireEvent.keyDown(screen.getByDisplayValue("Rule"), { key: "Escape" });
    expect(onSave).not.toHaveBeenCalled();
  });

  it("Switch toggles", () => {
    const onChange = vi.fn();
    render(<Switch checked={false} onChange={onChange} label="x" />);
    fireEvent.click(screen.getByRole("switch"));
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("badges show tier and fit", () => {
    render(<><TierBadge tier="cheap_cloud" /><FitBadge fit="wont_fit" /></>);
    expect(screen.getByText("Cheap cloud")).toBeInTheDocument();
    expect(screen.getByText("Won't fit")).toBeInTheDocument();
  });
});
