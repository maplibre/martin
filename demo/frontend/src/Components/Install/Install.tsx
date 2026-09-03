import { useState } from 'react';
import Command from './Command';
import Container from './Container';
import Tab from './Tab';
import Tabs from './Tabs';

const OPTIONS = [
  {
    command:
      'docker run -p 3000:3000 \\\n  -e DATABASE_URL=postgres://user:password@host/db \\\n  ghcr.io/maplibre/martin',
    id: 'docker',
    label: 'Docker',
  },
  {
    command: 'brew tap maplibre/martin\nbrew install martin',
    id: 'homebrew',
    label: 'Homebrew',
  },
  {
    command: 'cargo install cargo-binstall\ncargo binstall martin',
    id: 'binstall',
    label: 'cargo binstall',
  },
  {
    command: 'cargo install martin --locked',
    id: 'cargo',
    label: 'cargo install',
  },
  {
    command:
      'curl -O https://github.com/maplibre/martin/releases/latest/download/debian-x86_64.deb\nsudo dpkg -i ./debian-x86_64.deb',
    id: 'debian',
    label: 'Debian',
  },
];

const Install = () => {
  const [active, setActive] = useState(OPTIONS[0].id);
  const option = OPTIONS.find((o) => o.id === active) ?? OPTIONS[0];

  return (
    <Container>
      <Tabs>
        {OPTIONS.map((o) => (
          <Tab $active={o.id === active} key={o.id} onClick={() => setActive(o.id)} type="button">
            {o.label}
          </Tab>
        ))}
      </Tabs>
      <Command>{option.command}</Command>
    </Container>
  );
};

export default Install;
