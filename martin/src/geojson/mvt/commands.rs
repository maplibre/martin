/// TODO: attribution to geozero
/// Command to be executed and the number of times that the command will be executed
/// https://github.com/mapbox/vector-tile-spec/tree/master/2.1#431-command-integers
pub struct CommandInteger(pub u32);

#[derive(Debug)]
pub enum Command {
    MoveTo = 1,
    LineTo = 2,
    ClosePath = 7,
}

impl CommandInteger {
    pub fn from(id: Command, count: u32) -> u32 {
        ((id as u32) & 0x7) | (count << 3)
    }
    pub fn id(&self) -> u32 {
        self.0 & 0x7
    }
    pub fn count(&self) -> u32 {
        self.0 >> 3
    }
}

#[test]
fn test_commands() {
    assert_eq!(CommandInteger(9).id(), Command::MoveTo as u32);
    assert_eq!(CommandInteger(9).count(), 1);

    assert_eq!(CommandInteger::from(Command::MoveTo, 1), 9);
    assert_eq!(CommandInteger::from(Command::LineTo, 3), 26);
    assert_eq!(CommandInteger::from(Command::ClosePath, 1), 15);
}

/// Commands requiring parameters are followed by a ParameterInteger for each parameter required by that command
/// https://github.com/mapbox/vector-tile-spec/tree/master/2.1#432-parameter-integers
pub struct ParameterInteger(pub u32);

impl ParameterInteger {
    pub fn from(value: i32) -> u32 {
        ((value << 1) ^ (value >> 31)) as u32
    }
    pub fn value(&self) -> i32 {
        ((self.0 >> 1) as i32) ^ (-((self.0 & 1) as i32))
    }
}

#[test]
fn test_parameters() {
    assert_eq!(ParameterInteger(50).value(), 25);
    assert_eq!(ParameterInteger::from(25), 50);
}

#[test]
fn test_sequence() {
    type CommandSequence = Vec<u32>;

    let mut seq = CommandSequence::new();
    seq.push(CommandInteger::from(Command::MoveTo, 1));
    seq.push(ParameterInteger::from(25));
    seq.push(ParameterInteger::from(17));
    assert_eq!(seq, &[9, 50, 34]);

    let mut seq2 = CommandSequence::new();
    seq2.push(CommandInteger::from(Command::MoveTo, 1));
    seq.append(&mut seq2);
    assert_eq!(seq, &[9, 50, 34, 9]);
}
