FROM debian:wheezy

ENV DEBIAN_FRONTEND noninteractive
ENV WSGI_PROCESSES 4
ENV WSGI_THREADS 1
ENV WSGI_MAX_REQUESTS 100000

EXPOSE 80
VOLUME ["/var/hg/htdocs", "/var/hg/repos"]

RUN apt-get update && apt-get -y install libapache2-mod-wsgi python-dev vim

# Install our own Apache site.
RUN a2dissite 000-default
ADD vhost.conf /etc/apache2/sites-available/hg
RUN a2ensite hg

ADD hgwebconfig /defaulthgwebconfig

ADD entrypoint.sh /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]

CMD ["/usr/sbin/apache2", "-DFOREGROUND"]
