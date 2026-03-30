/// A simple calculator for testing forza workflows.
///
/// This crate exists as a test fixture — forza integration tests create
/// issues that add features, fix bugs, or refactor this code. The crate
/// is intentionally simple so agent-driven changes are easy to verify.
pub mod calculator {
    /// Add two numbers.
    pub fn add(a: i32, b: i32) -> i32 {
        a + b
    }

    /// Subtract two numbers.
    pub fn subtract(a: i32, b: i32) -> i32 {
        a - b
    }

    /// Multiply two numbers.
    pub fn multiply(a: i32, b: i32) -> i32 {
        a * b
    }

    /// Divide two numbers. Returns None if divisor is zero.
    pub fn divide(a: i32, b: i32) -> Option<i32> {
        if b == 0 { None } else { Some(a / b) }
    }

    /// Compute the factorial of n. Returns None if n > 20 (would overflow u64).
    pub fn factorial(n: u64) -> Option<u64> {
        if n > 20 {
            return None;
        }
        Some((1..=n).product())
    }
}

#[cfg(test)]
mod tests {
    use super::calculator::*;

    #[test]
    fn test_add() {
        assert_eq!(add(2, 3), 5);
        assert_eq!(add(-1, 1), 0);
    }

    #[test]
    fn test_subtract() {
        assert_eq!(subtract(5, 3), 2);
    }

    #[test]
    fn test_multiply() {
        assert_eq!(multiply(4, 3), 12);
        assert_eq!(multiply(0, 100), 0);
    }

    #[test]
    fn test_divide() {
        assert_eq!(divide(10, 2), Some(5));
        assert_eq!(divide(10, 0), None);
    }

    #[test]
    fn test_factorial() {
        assert_eq!(factorial(0), Some(1));
        assert_eq!(factorial(1), Some(1));
        assert_eq!(factorial(5), Some(120));
        assert_eq!(factorial(10), Some(3628800));
        assert_eq!(factorial(20), Some(2432902008176640000));
        assert_eq!(factorial(21), None);
    }
}
