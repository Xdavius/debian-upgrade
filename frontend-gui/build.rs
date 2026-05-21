// Compile l'interface Slint au moment du build de la GUI.
fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");
}
